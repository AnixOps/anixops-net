//! The window: a sidebar with the six pages and the page that is chosen, in GTK 4 and libadwaita.
//!
//! The layout is libadwaita's own (a split view that folds into one column on a narrow window), the colours
//! are the system's with the brand's accent on top, and every sentence comes from the shared catalogue.
//!
//! The engine is not in this process. It runs in the daemon, which the window reaches over the daemon's
//! socket (`client-window`); the foot of the sidebar says whether the daemon answers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use adw::prelude::*;
use client_session::session::LocalSession;
use client_session::system_proxy::Desktop;
use client_window::{CoreLink, LinkOptions, LinkState, WindowEnd};
use gtk::{gdk, gio, glib};
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};
use serde_json::json;

use crate::groups::{self, Choice, Group};
use crate::launch::{Appearance, Launch};
use crate::overview::{self, Overview, Pending, Press};
use crate::overview_page::OverviewPage;
use crate::proxies::{self, Action, Busy, Proxies};
use crate::proxies_page::{Asked, ProxiesPage};
use crate::sections::{self, Section};
use crate::subscriptions::{self, Change, Subscription};
use crate::{Language, APP_ID};

const BRAND_LIGHT: &str = include_str!("../data/brand-light.css");
const BRAND_DARK: &str = include_str!("../data/brand-dark.css");

/// The usual size of the window, in logical pixels.
const DEFAULT_SIZE: (i32, i32) = (1080, 720);
/// Below this width the sidebar and the page no longer fit side by side.
const NARROW: f64 = 640.0;
/// How long the window is left to settle (first layout, icons, fonts) before it is drawn into a file.
const SETTLE: Duration = Duration::from_millis(800);
/// How often the overview is asked for again, in seconds.
const REFRESH_SECONDS: u32 = 1;
/// How long the core gets to say how things are. It only reads its own state for this.
const ASK_PATIENCE: Duration = Duration::from_secs(10);
/// How long the core gets to connect or disconnect: starting the engine can mean downloading rule lists.
const PRESS_PATIENCE: Duration = Duration::from_secs(120);
/// How long the core gets to test a list of nodes: a node that does not answer takes seconds, and a
/// subscription can have thousands.
const TEST_PATIENCE: Duration = Duration::from_secs(1800);

/// Set when the window was asked for a screenshot and could not make it.
static FAILED: AtomicBool = AtomicBool::new(false);

/// Shows the window and returns when it is closed. `false` means a screenshot was asked for and not made.
pub fn run(launch: Launch) -> bool {
    let mut flags = gio::ApplicationFlags::empty();
    if launch.screenshot.is_some() {
        // A screenshot is its own short-lived window, also while the user's window is open.
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
    // The options are read by `Launch`; GTK gets none of them.
    RelmApp::from_app(application)
        .with_args(vec![APP_ID.to_owned()])
        .run::<Shell>(launch);
    !FAILED.load(Ordering::SeqCst)
}

/// The language for this desktop session.
fn desktop_language() -> Language {
    glib::language_names()
        .first()
        .map(|name| Language::preferred(name))
        .unwrap_or(Language::English)
}

struct Shell {
    language: Language,
    /// The place of the page that is shown in [`sections::ALL`].
    current: usize,
    /// Whether the daemon answers.
    core: LinkState,
    /// Keeps looking for the daemon for as long as the window lives.
    link: Arc<CoreLink>,
    /// The overview as the daemon last described it; `None` while it does not answer.
    overview: Option<Overview>,
    /// A question about the overview is out; the next one waits for its answer.
    refreshing: Arc<AtomicBool>,
    /// Why the last refresh got no answer.
    trouble: Option<String>,
    /// What the user pressed that the daemon has not finished.
    pending: Pending,
    /// Why the last press did not work.
    failure: Option<String>,
    /// The node list as the daemon last described it; `None` while it does not answer.
    proxies: Option<Proxies>,
    /// What the user searches the node list for.
    query: String,
    /// A question about the node list is out; the next one waits for its answer.
    listing: Arc<AtomicBool>,
    /// Why the last question about the node list got no answer.
    list_trouble: Option<String>,
    /// What the user asked for on the proxies page that the daemon has not finished.
    busy: Busy,
    /// Why the last thing asked for on the proxies page did not work.
    list_failure: Option<String>,
    /// The subscriptions as the daemon last described them; `None` while it does not say.
    subscriptions: Option<Vec<Subscription>>,
    /// A question about the subscriptions is out; the next one waits for its answer.
    subscribing: Arc<AtomicBool>,
    /// The subscriptions the daemon is still downloading, renaming or removing.
    changing: Vec<String>,
    /// The policy groups as the daemon last described them; `None` while it does not say.
    groups: Option<Vec<Group>>,
    /// A question about the policy groups is out; the next one waits for its answer.
    grouping: Arc<AtomicBool>,
    /// The policy groups the daemon is still choosing a member for.
    choosing: Vec<String>,
    /// How many subscriptions the daemon took in for this window; the link's box empties at each.
    imports: u32,
    /// Counts the changes of the link to the daemon, so that the answer to something asked of a daemon
    /// that has gone since is not taken for the present one's.
    era: u32,
}

#[derive(Debug)]
enum Show {
    Page(usize),
    /// The link to the daemon changed.
    Core(LinkState),
    /// Time to ask the daemon how things are.
    Tick,
    /// The daemon's answer to that.
    Overview(Result<Overview, String>),
    /// The user pressed the overview's button.
    Press,
    /// The daemon finished what the button asked for, or could not.
    Pressed(Result<(), String>),
    /// The daemon's node list for a search.
    Proxies(String, Result<Proxies, String>),
    /// The user asked for something on the proxies page.
    Ask(Asked),
    /// The daemon finished that, or could not. The number is the [`Shell::era`] it was asked in.
    Done(u32, Action, Result<(), String>),
    /// The daemon's subscriptions.
    Subscriptions(Result<Vec<Subscription>, String>),
    /// The daemon finished what was asked of a subscription, or could not; the number as in `Done`. With
    /// it, the subscriptions as they are after that, if the daemon said.
    Changed(u32, Change, Result<(), String>, Option<Vec<Subscription>>),
    /// The daemon's policy groups.
    Groups(Result<Vec<Group>, String>),
    /// The daemon uses the member chosen for a policy group, or could not; the number as in `Done`. With
    /// it, the groups as they are after that, if the daemon said.
    Chose(u32, Choice, Result<(), String>, Option<Vec<Group>>),
}

struct ShellWidgets {
    pages: gtk::ListBox,
    footer: gtk::ListBox,
    content: adw::NavigationPage,
    /// The overview page, or the placeholder of a page that is not built yet.
    stack: gtk::Stack,
    overview: OverviewPage,
    proxies: ProxiesPage,
    /// The value of [`Shell::imports`] at which the link's box was last emptied.
    emptied: u32,
    status: adw::StatusPage,
    core: gtk::Label,
}

/// The names of the two things the page area shows.
const OVERVIEW_PAGE: &str = "overview";
const PROXIES_PAGE: &str = "proxies";
const PLACEHOLDER_PAGE: &str = "placeholder";

impl Shell {
    fn text(&self, id: &str) -> &'static str {
        client_strings::text(self.language, id)
    }

    fn core_answers(&self) -> bool {
        matches!(self.core, LinkState::Connected { .. })
    }

    /// The overview page for what is known now.
    fn overview_shown(&self) -> overview::Shown {
        overview::shown(
            self.language,
            self.core_answers(),
            self.overview.as_ref(),
            self.pending,
            self.failure.as_deref().or(self.trouble.as_deref()),
        )
    }

    /// The proxies page for what is known now.
    fn proxies_shown(&self) -> proxies::Shown {
        proxies::shown(
            self.language,
            self.core_answers(),
            self.proxies.as_ref(),
            self.busy,
            self.list_failure
                .as_deref()
                .or(self.list_trouble.as_deref()),
        )
    }

    /// Asks the daemon for the node list on another thread, while the proxies page is the one shown and
    /// unless an earlier question is still out.
    fn list(&self, sender: &ComponentSender<Self>) {
        if !self.core_answers()
            || sections::ALL[self.current].id != sections::PROXIES
            || self.listing.swap(true, Ordering::SeqCst)
        {
            return;
        }
        self.list_subscriptions(sender);
        self.list_groups(sender);
        let link = Arc::clone(&self.link);
        let listing = Arc::clone(&self.listing);
        let input = sender.input_sender().clone();
        let query = self.query.clone();
        thread::spawn(move || {
            let answer = link
                .ask(Proxies::COMMAND, Proxies::args(&query), ASK_PATIENCE)
                .map(|answer| Proxies::read(&answer));
            listing.store(false, Ordering::SeqCst);
            let _ = input.send(Show::Proxies(query, answer));
        });
    }

    /// Asks the daemon for the subscriptions on another thread, unless an earlier question is still out.
    fn list_subscriptions(&self, sender: &ComponentSender<Self>) {
        if self.subscribing.swap(true, Ordering::SeqCst) {
            return;
        }
        let link = Arc::clone(&self.link);
        let subscribing = Arc::clone(&self.subscribing);
        let input = sender.input_sender().clone();
        thread::spawn(move || {
            let answer = link
                .ask(subscriptions::COMMAND, json!({}), ASK_PATIENCE)
                .map(|answer| subscriptions::read(&answer));
            subscribing.store(false, Ordering::SeqCst);
            let _ = input.send(Show::Subscriptions(answer));
        });
    }

    /// Asks the daemon for the policy groups on another thread, unless an earlier question is still out.
    fn list_groups(&self, sender: &ComponentSender<Self>) {
        if self.grouping.swap(true, Ordering::SeqCst) {
            return;
        }
        let link = Arc::clone(&self.link);
        let grouping = Arc::clone(&self.grouping);
        let input = sender.input_sender().clone();
        thread::spawn(move || {
            let answer = link
                .ask(groups::COMMAND, json!({}), ASK_PATIENCE)
                .map(|answer| groups::read(&answer));
            grouping.store(false, Ordering::SeqCst);
            let _ = input.send(Show::Groups(answer));
        });
    }

    /// Tells the daemon which member the user chose for a policy group, on another thread.
    fn choose(&mut self, choice: Choice, sender: &ComponentSender<Self>) {
        self.choosing.push(choice.group.clone());
        self.list_failure = None;
        let link = Arc::clone(&self.link);
        let input = sender.input_sender().clone();
        let era = self.era;
        thread::spawn(move || {
            let (command, args) = choice.ask();
            let answer = link.ask(command, args, PRESS_PATIENCE).map(|_| ());
            // What the row says after this, before a member can be chosen in it again.
            let after = link
                .ask(groups::COMMAND, json!({}), ASK_PATIENCE)
                .map(|answer| groups::read(&answer))
                .ok();
            let _ = input.send(Show::Chose(era, choice, answer, after));
        });
    }

    /// Sends what the user asked of a subscription to the daemon, on another thread.
    fn change(&mut self, change: Change, sender: &ComponentSender<Self>) {
        self.changing.push(change.id().to_owned());
        self.list_failure = None;
        let link = Arc::clone(&self.link);
        let input = sender.input_sender().clone();
        let era = self.era;
        thread::spawn(move || {
            let (command, args) = change.ask();
            let answer = link.ask(command, args, PRESS_PATIENCE).map(|_| ());
            // What the row says after this, before it can be asked again.
            let after = link
                .ask(subscriptions::COMMAND, json!({}), ASK_PATIENCE)
                .map(|answer| subscriptions::read(&answer))
                .ok();
            let _ = input.send(Show::Changed(era, change, answer, after));
        });
    }

    /// Sends what the user asked for on the proxies page to the daemon, on another thread.
    fn act(&mut self, action: Action, sender: &ComponentSender<Self>) {
        self.busy.set(&action, true);
        self.list_failure = None;
        let link = Arc::clone(&self.link);
        let input = sender.input_sender().clone();
        let era = self.era;
        thread::spawn(move || {
            let (command, args) = action.ask();
            let patience = match action {
                Action::Test(_) => TEST_PATIENCE,
                Action::Import(_) | Action::Switch(_) => PRESS_PATIENCE,
            };
            let answer = link.ask(command, args, patience).map(|_| ());
            let _ = input.send(Show::Done(era, action, answer));
        });
    }

    /// Asks the daemon for the overview on another thread, unless an earlier question is still out.
    fn refresh(&self, sender: &ComponentSender<Self>) {
        if !self.core_answers() || self.refreshing.swap(true, Ordering::SeqCst) {
            return;
        }
        let link = Arc::clone(&self.link);
        let refreshing = Arc::clone(&self.refreshing);
        let input = sender.input_sender().clone();
        thread::spawn(move || {
            let answer = link
                .ask(Overview::COMMAND, json!({}), ASK_PATIENCE)
                .map(|answer| Overview::read(&answer));
            refreshing.store(false, Ordering::SeqCst);
            let _ = input.send(Show::Overview(answer));
        });
    }

    /// Sends the button's command to the daemon on another thread.
    fn press(&mut self, press: Press, sender: &ComponentSender<Self>) {
        self.pending = press.pending();
        self.failure = None;
        let link = Arc::clone(&self.link);
        let input = sender.input_sender().clone();
        thread::spawn(move || {
            let answer = link
                .ask(press.command(), json!({}), PRESS_PATIENCE)
                .map(|_| ());
            let _ = input.send(Show::Pressed(answer));
        });
    }

    /// The line at the foot of the sidebar. Why the daemon does not answer is for the log, not for here.
    fn core_text(&self) -> String {
        match &self.core {
            LinkState::Connecting => self.text("core.connecting").to_owned(),
            LinkState::Connected { core_version } => {
                client_strings::format(self.language, "core.connected", &[core_version.as_str()])
            }
            LinkState::Unavailable { .. } => self.text("core.unavailable").to_owned(),
        }
    }
}

/// Starts looking for the daemon. The window answers what the daemon asks of this user's session (the
/// desktop's proxy settings); each change of the link comes back to the window as [`Show::Core`].
fn link(socket: Option<String>, sender: &ComponentSender<Shell>) -> CoreLink {
    let desktop = Desktop::from_env();
    let executable =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("anixops-net-preview"));
    // No login entry while this is the preview: the window that starts after login is still the old one.
    let session = LocalSession::new(desktop, None, executable);
    // What the daemon tells the window without being asked (notifications) is shown once the pages are.
    let end = WindowEnd::new(desktop, Box::new(session), |_, _| {});
    let mut options = LinkOptions::new(env!("CARGO_PKG_VERSION"));
    if let Some(socket) = socket {
        options.endpoint = socket;
    }
    let input = sender.input_sender().clone();
    CoreLink::start(options, Arc::new(end), move |state| {
        // The window is gone when this fails, and the link with it.
        let _ = input.send(Show::Core(state.clone()));
    })
}

/// One row of the sidebar: the section's icon and name.
fn row(section: &Section, name: &str) -> gtk::ListBoxRow {
    let icon = gtk::Image::from_icon_name(section.icon);
    // The name beside it says what the row is; the icon adds nothing for a screen reader.
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    let label = gtk::Label::new(Some(name));
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.append(&icon);
    line.append(&label);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    row.set_widget_name(&format!("nav-{}", section.id));
    row.update_property(&[gtk::accessible::Property::Label(name)]);
    row
}

/// The list a section's row is in, and the row's place in that list.
fn place(index: usize) -> (bool, i32) {
    let footer = sections::ALL[index].footer;
    let before = sections::ALL[..index]
        .iter()
        .filter(|section| section.footer == footer)
        .count();
    (footer, before as i32)
}

/// The section behind a row of one of the two lists.
fn section_at(footer: bool, row: i32) -> Option<usize> {
    sections::ALL
        .iter()
        .enumerate()
        .filter(|(_, section)| section.footer == footer)
        .nth(usize::try_from(row).ok()?)
        .map(|(index, _)| index)
}

impl SimpleComponent for Shell {
    type Init = Launch;
    type Input = Show;
    type Output = ();
    type Root = adw::ApplicationWindow;
    type Widgets = ShellWidgets;

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .default_width(DEFAULT_SIZE.0)
            .default_height(DEFAULT_SIZE.1)
            .width_request(360)
            .height_request(294)
            .build()
    }

    fn init(
        launch: Self::Init,
        window: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Shell {
            language: launch.language.unwrap_or_else(desktop_language),
            current: sections::position(launch.page).unwrap_or(0),
            core: LinkState::Connecting,
            link: Arc::new(link(launch.socket, &sender)),
            overview: None,
            refreshing: Arc::new(AtomicBool::new(false)),
            trouble: None,
            pending: Pending::Nothing,
            failure: None,
            proxies: None,
            query: String::new(),
            listing: Arc::new(AtomicBool::new(false)),
            list_trouble: None,
            busy: Busy::default(),
            list_failure: None,
            subscriptions: None,
            subscribing: Arc::new(AtomicBool::new(false)),
            changing: Vec::new(),
            groups: None,
            grouping: Arc::new(AtomicBool::new(false)),
            choosing: Vec::new(),
            imports: 0,
            era: 0,
        };

        let style = adw::StyleManager::default();
        style.set_color_scheme(match launch.appearance {
            Appearance::System => adw::ColorScheme::Default,
            Appearance::Light => adw::ColorScheme::ForceLight,
            Appearance::Dark => adw::ColorScheme::ForceDark,
        });
        brand(&style);

        window.set_title(Some(model.text("app.name")));
        if let Some((width, height)) = launch.size {
            window.set_default_size(width, height);
        }

        // The sidebar: the pages, and settings at the bottom.
        let pages = gtk::ListBox::new();
        let footer = gtk::ListBox::new();
        for list in [&pages, &footer] {
            list.set_selection_mode(gtk::SelectionMode::Single);
            list.add_css_class("navigation-sidebar");
            list.update_property(&[gtk::accessible::Property::Label(model.text("nav.label"))]);
        }
        for section in &sections::ALL {
            let list = if section.footer { &footer } else { &pages };
            list.append(&row(section, model.text(section.title)));
        }
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&pages));

        let sidebar_bar = adw::HeaderBar::new();
        sidebar_bar.set_title_widget(Some(&adw::WindowTitle::new(
            model.text("app.name"),
            model.text("app.preview"),
        )));
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_top_bar(&sidebar_bar);
        sidebar_view.set_content(Some(&scroller));
        // Under settings, in small print: whether the daemon answers.
        let core = gtk::Label::new(None);
        core.set_widget_name("core-status");
        core.set_xalign(0.0);
        core.set_wrap(true);
        core.add_css_class("caption");
        core.add_css_class("dim-label");
        core.set_margin_start(12);
        core.set_margin_end(12);
        core.set_margin_top(4);
        core.set_margin_bottom(10);
        let foot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        foot.append(&footer);
        foot.append(&core);
        sidebar_view.add_bottom_bar(&foot);
        let sidebar = adw::NavigationPage::new(&sidebar_view, model.text("app.name"));

        // The page: the overview, or for a page that is not built yet, what it will hold.
        let overview = OverviewPage::new(model.language, {
            let sender = sender.clone();
            move || sender.input(Show::Press)
        });
        let proxies = ProxiesPage::new(model.language, {
            let sender = sender.clone();
            move |asked| sender.input(Show::Ask(asked))
        });
        let status = adw::StatusPage::new();
        status.set_widget_name("page");
        let stack = gtk::Stack::new();
        stack.add_named(overview.widget(), Some(OVERVIEW_PAGE));
        stack.add_named(proxies.widget(), Some(PROXIES_PAGE));
        stack.add_named(&status, Some(PLACEHOLDER_PAGE));
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&adw::HeaderBar::new());
        content_view.set_content(Some(&stack));
        let content = adw::NavigationPage::new(&content_view, "");

        let split = adw::NavigationSplitView::new();
        split.set_min_sidebar_width(200.0);
        split.set_max_sidebar_width(280.0);
        split.set_sidebar(Some(&sidebar));
        split.set_content(Some(&content));
        window.set_content(Some(&split));

        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            NARROW,
            adw::LengthUnit::Sp,
        ));
        narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(narrow);

        for (list, is_footer) in [(&pages, false), (&footer, true)] {
            list.connect_row_selected({
                let sender = sender.clone();
                move |_, row| {
                    if let Some(index) = row.and_then(|row| section_at(is_footer, row.index())) {
                        sender.input(Show::Page(index));
                    }
                }
            });
            // On a narrow window the sidebar and the page take turns: choosing a page moves on to it, also
            // when it is the page that was already chosen.
            list.connect_row_activated({
                let split = split.clone();
                move |_, _| split.set_show_content(true)
            });
        }

        let mut widgets = ShellWidgets {
            pages,
            footer,
            content,
            stack,
            overview,
            proxies,
            emptied: 0,
            status,
            core,
        };
        // The overview follows the daemon for as long as the window lives.
        glib::timeout_add_seconds_local(REFRESH_SECONDS, {
            let sender = sender.clone();
            move || {
                sender.input(Show::Tick);
                glib::ControlFlow::Continue
            }
        });
        model.update_view(&mut widgets, sender);
        // The keyboard starts on the chosen page's row. Left to itself the window would focus the first row
        // of the sidebar, and a list selects the row the keyboard lands on: `--page` would open on Overview.
        let (in_footer, at) = place(model.current);
        let chosen = if in_footer {
            &widgets.footer
        } else {
            &widgets.pages
        }
        .row_at_index(at);
        gtk::prelude::GtkWindowExt::set_focus(&window, chosen.as_ref());

        if launch.subscriptions {
            widgets.proxies.open_subscriptions();
        }
        if launch.groups {
            widgets.proxies.open_groups();
        }
        if let Some(file) = launch.screenshot {
            // The page the screenshot is of, not the sidebar, when only one of them fits.
            split.set_show_content(true);
            let window = window.clone();
            glib::timeout_add_local_once(SETTLE, move || {
                if let Err(problem) = draw(&window, &file) {
                    eprintln!(
                        "anixops-net-preview: no screenshot at {}: {problem}",
                        file.display()
                    );
                    FAILED.store(true, Ordering::SeqCst);
                }
                // Not `close`: with a dialog over the window, that closes the dialog and leaves the
                // window open.
                window.destroy();
            });
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            Show::Page(index) => {
                self.current = index;
                self.list(&sender);
            }
            Show::Core(state) => {
                self.core = state;
                // Whatever was known belongs to the connection that was; a daemon that comes back may be
                // another version with other nodes.
                self.overview = None;
                self.trouble = None;
                self.failure = None;
                self.pending = Pending::Nothing;
                self.proxies = None;
                self.list_trouble = None;
                self.list_failure = None;
                self.busy = Busy::default();
                self.subscriptions = None;
                self.changing.clear();
                self.groups = None;
                self.choosing.clear();
                self.era = self.era.wrapping_add(1);
                self.refresh(&sender);
                self.list(&sender);
            }
            Show::Tick => {
                self.refresh(&sender);
                self.list(&sender);
            }
            // An answer that comes after the daemon went away says nothing about now.
            Show::Overview(_) if !self.core_answers() => {}
            Show::Overview(Ok(overview)) => {
                self.overview = Some(overview);
                self.trouble = None;
            }
            Show::Overview(Err(problem)) => {
                self.overview = None;
                self.trouble = Some(problem);
            }
            Show::Press => {
                let shown = self.overview_shown();
                if shown.enabled {
                    self.press(shown.press, &sender);
                }
            }
            Show::Pressed(answer) => {
                self.pending = Pending::Nothing;
                self.failure = answer.err();
                // Ask again at once, not at the next tick.
                self.refresh(&sender);
            }
            // An answer that comes after the daemon went away says nothing about now.
            Show::Proxies(..) if !self.core_answers() => {}
            // An answer to a search the user has typed past: ask for the one that stands now.
            Show::Proxies(query, _) if query != self.query => self.list(&sender),
            Show::Proxies(_, Ok(proxies)) => {
                self.proxies = Some(proxies);
                self.list_trouble = None;
            }
            Show::Proxies(_, Err(problem)) => {
                self.proxies = None;
                self.list_trouble = Some(problem);
            }
            Show::Ask(Asked::Search(query)) => {
                self.query = query;
                self.list(&sender);
            }
            Show::Ask(Asked::Change(change)) => {
                // One thing at a time is asked of a subscription.
                if self.core_answers() && !self.changing.iter().any(|id| id == change.id()) {
                    self.change(change, &sender);
                }
            }
            Show::Ask(Asked::Choose(choice)) => {
                // One member at a time is chosen for a group.
                if self.core_answers() && !self.choosing.contains(&choice.group) {
                    self.choose(choice, &sender);
                }
            }
            Show::Ask(asked) => {
                let shown = self.proxies_shown();
                let action = match asked {
                    Asked::Import(location) if shown.can_import => Some(Action::Import(location)),
                    Asked::Test if shown.can_test => Some(Action::Test(self.query.clone())),
                    // Choosing the node that is already in use asks for nothing.
                    Asked::Switch(id)
                        if shown.can_switch
                            && !shown.rows.iter().any(|row| row.selected && row.id == id) =>
                    {
                        Some(Action::Switch(id))
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    self.act(action, &sender);
                }
            }
            // The daemon that was asked is gone; what it was busy with went with it.
            Show::Done(era, ..) if era != self.era => {}
            Show::Done(_, action, answer) => {
                self.busy.set(&action, false);
                if matches!(action, Action::Import(_)) && answer.is_ok() {
                    self.imports += 1;
                }
                self.list_failure = answer.err();
                // Ask again at once, not at the next tick: the list, and the overview's node in use.
                self.list(&sender);
                self.refresh(&sender);
            }
            // An answer that comes after the daemon went away says nothing about now.
            Show::Subscriptions(_) if !self.core_answers() => {}
            // An answer that did not come leaves what was known; the question goes out again.
            Show::Subscriptions(Err(_)) => {}
            Show::Subscriptions(Ok(subscriptions)) => self.subscriptions = Some(subscriptions),
            Show::Groups(_) if !self.core_answers() => {}
            Show::Groups(Err(_)) => {}
            Show::Groups(Ok(groups)) => self.groups = Some(groups),
            Show::Chose(era, ..) if era != self.era => {}
            Show::Chose(_, choice, answer, after) => {
                self.choosing.retain(|group| *group != choice.group);
                if after.is_some() {
                    self.groups = after;
                }
                self.list_failure = answer.err();
                // The nodes and the overview's node in use, as they are after this.
                self.list(&sender);
                self.refresh(&sender);
            }
            Show::Changed(era, ..) if era != self.era => {}
            Show::Changed(_, change, answer, after) => {
                self.changing.retain(|id| id != change.id());
                if after.is_some() {
                    self.subscriptions = after;
                }
                self.list_failure = answer.err();
                // The subscriptions, their nodes, and the overview's node in use, as they are after this.
                self.list(&sender);
                self.refresh(&sender);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        let section = &sections::ALL[self.current];
        let (in_footer, at) = place(self.current);
        let (list, other) = if in_footer {
            (&widgets.footer, &widgets.pages)
        } else {
            (&widgets.pages, &widgets.footer)
        };
        // Only one row of the two lists is the chosen one.
        other.unselect_all();
        let row = list.row_at_index(at);
        if list.selected_row() != row {
            list.select_row(row.as_ref());
        }

        widgets.content.set_title(self.text(section.title));
        widgets.overview.show(&self.overview_shown());
        let subscriptions = self.subscriptions.as_ref().map(|subscriptions| {
            subscriptions::shown(self.language, subscriptions, &self.changing)
        });
        let groups = self
            .groups
            .as_ref()
            .map(|groups| groups::shown(self.language, groups, &self.choosing));
        widgets.proxies.show(
            &self.proxies_shown(),
            self.busy.switching,
            subscriptions.as_deref(),
            groups.as_deref(),
        );
        if widgets.emptied != self.imports {
            widgets.proxies.clear_link();
            widgets.emptied = self.imports;
        }
        widgets.stack.set_visible_child_name(match section.id {
            sections::OVERVIEW => OVERVIEW_PAGE,
            sections::PROXIES => PROXIES_PAGE,
            _ => PLACEHOLDER_PAGE,
        });
        widgets.status.set_icon_name(Some(section.icon));
        widgets.status.set_title(self.text(section.title));
        let description = format!(
            "{}\n{}",
            glib::markup_escape_text(self.text(section.summary)),
            glib::markup_escape_text(self.text("page.unfinished"))
        );
        widgets.status.set_description(Some(&description));
        widgets.core.set_label(&self.core_text());
    }
}

/// Puts the brand's accent over libadwaita's colours, for the scheme that is showing. High contrast keeps
/// the system's colours.
fn brand(style: &adw::StyleManager) {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let apply = move |style: &adw::StyleManager| {
        provider.load_from_string(if style.is_high_contrast() {
            ""
        } else if style.is_dark() {
            BRAND_DARK
        } else {
            BRAND_LIGHT
        });
    };
    apply(style);
    style.connect_dark_notify(apply.clone());
    style.connect_high_contrast_notify(apply);
}

/// Draws the window as it is now into a PNG file, with the window's own renderer.
fn draw(window: &adw::ApplicationWindow, file: &std::path::Path) -> Result<(), String> {
    let (width, height) = (window.width(), window.height());
    if width <= 0 || height <= 0 {
        return Err("the window has no size yet".to_owned());
    }
    let snapshot = gtk::Snapshot::new();
    // One file pixel per device pixel: on a display scaled by two the picture has twice the pixels.
    let scale = window.scale_factor().max(1) as f32;
    snapshot.scale(scale, scale);
    gtk::WidgetPaintable::new(Some(window)).snapshot(
        &snapshot,
        f64::from(width),
        f64::from(height),
    );
    let node = snapshot.to_node().ok_or("the window drew nothing")?;
    let renderer = window.renderer().ok_or("the window has no renderer")?;
    let texture = renderer.render_texture(node, None);
    if let Some(folder) = file
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    texture.save_to_png(file).map_err(|error| error.to_string())
}
