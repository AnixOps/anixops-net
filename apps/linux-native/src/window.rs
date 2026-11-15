//! The window: a sidebar with the six pages and the page that is chosen, in GTK 4 and libadwaita.
//!
//! The layout is libadwaita's own (a split view that folds into one column on a narrow window), the colours
//! are the system's with the brand's accent on top, and every sentence comes from the shared catalogue.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};

use crate::launch::{Appearance, Launch};
use crate::sections::{self, Section};
use crate::{Language, APP_ID};

const BRAND_LIGHT: &str = include_str!("../data/brand-light.css");
const BRAND_DARK: &str = include_str!("../data/brand-dark.css");

/// The usual size of the window, in logical pixels.
const DEFAULT_SIZE: (i32, i32) = (1080, 720);
/// Below this width the sidebar and the page no longer fit side by side.
const NARROW: f64 = 640.0;
/// How long the window is left to settle (first layout, icons, fonts) before it is drawn into a file.
const SETTLE: Duration = Duration::from_millis(800);

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
}

#[derive(Debug)]
enum Show {
    Page(usize),
}

struct ShellWidgets {
    pages: gtk::ListBox,
    footer: gtk::ListBox,
    content: adw::NavigationPage,
    status: adw::StatusPage,
}

impl Shell {
    fn text(&self, id: &str) -> &'static str {
        client_strings::text(self.language, id)
    }
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
        sidebar_view.add_bottom_bar(&footer);
        let sidebar = adw::NavigationPage::new(&sidebar_view, model.text("app.name"));

        // The page. Until the real pages are built, each one says what it will hold.
        let status = adw::StatusPage::new();
        status.set_widget_name("page");
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&adw::HeaderBar::new());
        content_view.set_content(Some(&status));
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
            status,
        };
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
                window.close();
            });
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            Show::Page(index) => self.current = index,
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
        widgets.status.set_icon_name(Some(section.icon));
        widgets.status.set_title(self.text(section.title));
        let description = format!(
            "{}\n{}",
            glib::markup_escape_text(self.text(section.summary)),
            glib::markup_escape_text(self.text("page.unfinished"))
        );
        widgets.status.set_description(Some(&description));
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
