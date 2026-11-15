//! The proxies page's widgets: where a subscription is imported, the search, the delay test, and the list of
//! nodes under their subscriptions. What they say is decided in [`crate::proxies`]; this only draws it.
//!
//! The list is a `GtkListView`: only the rows near the viewport exist as widgets, so a subscription with
//! thousands of nodes costs no more than one with ten. The row that is selected is the node in use;
//! selecting another row asks the core to use that node.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::group_list::GroupList;
use crate::groups::{self, Choice};
use crate::proxies::{Row, Shown, Tone};
use crate::subscription_list::SubscriptionList;
use crate::subscriptions::{self, Change};
use crate::Language;

/// The widest the page's column gets, in logical pixels.
const COLUMN: i32 = 720;

/// The names of the two things under the controls.
const LIST: &str = "list";
const NOTICE: &str = "notice";

/// What the user asked for on the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Import what is at this location.
    Import(String),
    /// Show the nodes this search leaves.
    Search(String),
    /// Test the delay of the nodes shown.
    Test,
    /// Use the node with this id.
    Switch(String),
    /// Download a subscription again, give it another name or remove it.
    Change(Change),
    /// Use this member in a policy group.
    Choose(Choice),
}

pub struct ProxiesPage {
    root: gtk::Box,
    link: gtk::Entry,
    import: gtk::Button,
    test: gtk::Button,
    count: gtk::Label,
    manage: gtk::Button,
    subscriptions: Rc<SubscriptionList>,
    choose: gtk::Button,
    groups: Rc<GroupList>,
    /// Whether the core said which policy groups there are the last time the page was drawn.
    grouped: Cell<bool>,
    /// Whether the core said which subscriptions there are the last time the page was drawn.
    listed: Cell<bool>,
    error: gtk::Label,
    stack: gtk::Stack,
    notice: adw::StatusPage,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    /// The rows the list holds now, to change only what differs.
    drawn: RefCell<Vec<Row>>,
    /// Set while the page itself changes the list, so that this is not taken for the user choosing a node.
    syncing: Rc<Cell<bool>>,
}

/// The row behind an item of the list.
fn row_of(item: Option<glib::Object>) -> Option<Row> {
    let boxed = item.and_downcast::<glib::BoxedAnyObject>()?;
    let row = boxed.borrow::<Row>().clone();
    Some(row)
}

/// Makes the widgets of one node's row: a mark for the node in use, the name over the protocol, the delay.
fn build_row(item: &gtk::ListItem) {
    let mark = gtk::Image::from_icon_name("object-select-symbolic");
    // The row's description says "in use"; the mark adds nothing for a screen reader.
    mark.set_accessible_role(gtk::AccessibleRole::Presentation);
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let protocol = gtk::Label::new(None);
    protocol.set_xalign(0.0);
    protocol.set_ellipsize(gtk::pango::EllipsizeMode::End);
    protocol.add_css_class("caption");
    protocol.add_css_class("dim-label");
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    texts.append(&label);
    texts.append(&protocol);
    let delay = gtk::Label::new(None);
    delay.add_css_class("numeric");
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.append(&mark);
    line.append(&texts);
    line.append(&delay);
    item.set_child(Some(&line));
}

/// Writes a node into the widgets [`build_row`] made.
fn fill_row(item: &gtk::ListItem, language: Language) {
    let Some(row) = row_of(item.item()) else {
        return;
    };
    let Some(mark) = item.child().and_then(|line| line.first_child()) else {
        return;
    };
    let Some(texts) = mark.next_sibling() else {
        return;
    };
    let label = texts.first_child().and_downcast::<gtk::Label>();
    let protocol = label
        .as_ref()
        .and_then(|label| label.next_sibling())
        .and_downcast::<gtk::Label>();
    let delay = texts.next_sibling().and_downcast::<gtk::Label>();
    let (Some(label), Some(protocol), Some(delay)) = (label, protocol, delay) else {
        return;
    };

    // Hidden, not removed: the names stay in one column whether or not a node is in use.
    mark.set_opacity(if row.selected { 1.0 } else { 0.0 });
    // Names of nodes are shown as they are written, not read as markup.
    label.set_label(&row.label);
    protocol.set_label(&row.protocol);
    protocol.set_visible(!row.protocol.is_empty());
    delay.set_label(&row.delay);
    for class in ["success", "warning", "error"] {
        delay.remove_css_class(class);
    }
    match row.tone {
        Tone::Plain => {}
        Tone::Fast => delay.add_css_class("success"),
        Tone::Slow => delay.add_css_class("warning"),
        Tone::Failed => delay.add_css_class("error"),
    }
    item.set_accessible_label(&row.label);
    item.set_accessible_description(&row.description(language));
}

impl ProxiesPage {
    /// Builds the page. `asked` is called with what the user asks for.
    pub fn new(language: Language, asked: impl Fn(Asked) + Clone + 'static) -> Self {
        let text = |id: &str| client_strings::text(language, id);

        // Where a subscription comes in.
        let link_caption = gtk::Label::new(Some(text("proxies.import.link")));
        link_caption.set_xalign(0.0);
        link_caption.add_css_class("caption-heading");
        let link = gtk::Entry::new();
        link.set_hexpand(true);
        link.set_input_purpose(gtk::InputPurpose::Url);
        link.update_property(&[gtk::accessible::Property::Label(text(
            "proxies.import.link",
        ))]);
        let import = gtk::Button::new();
        import.set_widget_name("import");
        let import_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        import_line.append(&link);
        import_line.append(&import);
        let send_link = {
            let asked = asked.clone();
            let link = link.clone();
            move || {
                let location = link.text().trim().to_owned();
                if !location.is_empty() {
                    asked(Asked::Import(location));
                }
            }
        };
        import.connect_clicked({
            let send_link = send_link.clone();
            move |_| send_link()
        });
        link.connect_activate(move |_| send_link());

        // The search and the delay test.
        let search = gtk::SearchEntry::new();
        search.set_hexpand(true);
        search.set_placeholder_text(Some(text("proxies.search")));
        search.update_property(&[gtk::accessible::Property::Label(text("proxies.search"))]);
        search.connect_search_changed({
            let asked = asked.clone();
            move |search| asked(Asked::Search(search.text().to_string()))
        });
        let test = gtk::Button::new();
        test.set_widget_name("test");
        test.connect_clicked({
            let asked = asked.clone();
            move |_| asked(Asked::Test)
        });
        let search_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        search_line.append(&search);
        search_line.append(&test);

        let count = gtk::Label::new(None);
        count.set_xalign(0.0);
        count.set_hexpand(true);
        count.add_css_class("caption");
        count.add_css_class("dim-label");
        // The subscriptions the nodes come from, in a dialog over the page.
        let manage = gtk::Button::with_label(text("proxies.subscriptions"));
        manage.set_widget_name("subscriptions");
        manage.add_css_class("flat");
        let subscriptions = Rc::new(SubscriptionList::new(language, {
            let asked = asked.clone();
            move |change| asked(Asked::Change(change))
        }));
        manage.connect_clicked({
            let subscriptions = Rc::clone(&subscriptions);
            move |button| subscriptions.present(button)
        });
        // The policy groups of the subscription in use, in a dialog of their own. Most subscriptions have
        // none; the button is only there when there are some.
        let choose = gtk::Button::with_label(text("proxies.groups"));
        choose.set_widget_name("groups");
        choose.add_css_class("flat");
        choose.set_visible(false);
        let groups = Rc::new(GroupList::new(language, {
            let asked = asked.clone();
            move |choice| asked(Asked::Choose(choice))
        }));
        choose.connect_clicked({
            let groups = Rc::clone(&groups);
            move |button| groups.present(button)
        });
        let count_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        count_line.append(&count);
        count_line.append(&choose);
        count_line.append(&manage);
        let error = gtk::Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.add_css_class("error");
        // So that the reason can be copied into a message asking for help.
        error.set_selectable(true);
        error.set_visible(false);

        let controls = gtk::Box::new(gtk::Orientation::Vertical, 8);
        controls.set_margin_top(12);
        controls.set_margin_bottom(8);
        controls.set_margin_start(16);
        controls.set_margin_end(16);
        controls.append(&link_caption);
        controls.append(&import_line);
        controls.append(&search_line);
        controls.append(&count_line);
        controls.append(&error);
        let controls_clamp = adw::Clamp::new();
        controls_clamp.set_maximum_size(COLUMN);
        controls_clamp.set_child(Some(&controls));

        // The nodes. Each run of nodes of one subscription is a section with the subscription's name over
        // it; the core lists the nodes run by run, so ordering by section moves nothing.
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let sections = gtk::SortListModel::new(Some(store.clone()), None::<gtk::Sorter>);
        let by_section = gtk::CustomSorter::new(|a, b| {
            let section = |item: &glib::Object| {
                item.downcast_ref::<glib::BoxedAnyObject>()
                    .map_or(0, |boxed| boxed.borrow::<Row>().section)
            };
            section(a).cmp(&section(b)).into()
        });
        sections.set_section_sorter(Some(&by_section));
        let selection = gtk::SingleSelection::new(Some(sections));
        // No node is chosen for the user: the selected row is the node the core uses, or none.
        selection.set_autoselect(false);
        selection.set_can_unselect(true);

        let rows = gtk::SignalListItemFactory::new();
        rows.connect_setup(|_, object| {
            if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
                build_row(item);
            }
        });
        rows.connect_bind(move |_, object| {
            if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
                fill_row(item, language);
            }
        });
        let headings = gtk::SignalListItemFactory::new();
        headings.connect_setup(|_, object| {
            if let Some(header) = object.downcast_ref::<gtk::ListHeader>() {
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.add_css_class("heading");
                label.set_margin_top(12);
                label.set_margin_bottom(4);
                label.set_margin_start(12);
                label.set_margin_end(12);
                header.set_child(Some(&label));
            }
        });
        headings.connect_bind(|_, object| {
            let Some(header) = object.downcast_ref::<gtk::ListHeader>() else {
                return;
            };
            if let (Some(label), Some(row)) = (
                header.child().and_downcast::<gtk::Label>(),
                row_of(header.item()),
            ) {
                label.set_label(&row.heading);
            }
        });

        let list = gtk::ListView::new(Some(selection.clone()), Some(rows));
        list.set_header_factory(Some(&headings));
        list.set_widget_name("nodes");
        list.add_css_class("rich-list");
        list.update_property(&[gtk::accessible::Property::Label(text("proxies.list"))]);
        let syncing = Rc::new(Cell::new(false));
        selection.connect_selection_changed({
            let syncing = Rc::clone(&syncing);
            move |selection, _, _| {
                if syncing.get() {
                    return;
                }
                if let Some(row) = row_of(selection.selected_item()) {
                    asked(Asked::Switch(row.id));
                }
            }
        });
        let list_clamp = adw::ClampScrollable::new();
        list_clamp.set_maximum_size(COLUMN);
        list_clamp.set_child(Some(&list));
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&list_clamp));

        let notice = adw::StatusPage::new();
        notice.set_icon_name(Some("network-server-symbolic"));
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&scroller, Some(LIST));
        stack.add_named(&notice, Some(NOTICE));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&controls_clamp);
        root.append(&stack);

        Self {
            root,
            link,
            import,
            test,
            count,
            manage,
            subscriptions,
            listed: Cell::new(false),
            choose,
            groups,
            grouped: Cell::new(false),
            error,
            stack,
            notice,
            store,
            selection,
            drawn: RefCell::new(Vec::new()),
            syncing,
        }
    }

    /// The page, to put into the window.
    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Opens the list of subscriptions over the window once the window is up, as pressing the page's
    /// button does.
    pub fn open_subscriptions(&self) {
        let subscriptions = Rc::clone(&self.subscriptions);
        let root = self.root.clone();
        glib::idle_add_local_once(move || subscriptions.present(&root));
    }

    /// Opens the list of policy groups over the window once the window is up, as pressing the page's
    /// button does.
    pub fn open_groups(&self) {
        let groups = Rc::clone(&self.groups);
        let root = self.root.clone();
        glib::idle_add_local_once(move || groups.present(&root));
    }

    /// Empties the box a subscription's link is written in, once the core has taken the link.
    pub fn clear_link(&self) {
        self.link.set_text("");
    }

    /// Draws what is known now. While the core is switching nodes the row the user chose stays selected,
    /// whatever the core still says is in use. `subscriptions` is `None` while the core does not say which
    /// there are: there is then nothing to open, and what was open is closed. The same goes for `groups`.
    pub fn show(
        &self,
        shown: &Shown,
        switching: bool,
        subscriptions: Option<&[subscriptions::Row]>,
        groups: Option<&[groups::Row]>,
    ) {
        self.choose
            .set_visible(groups.is_some_and(|rows| !rows.is_empty()));
        match groups {
            Some(rows) => {
                // A subscription without groups came into use: there is nothing left to choose in.
                if rows.is_empty() {
                    self.groups.close();
                }
                self.groups.show(rows);
            }
            None if self.grouped.get() => {
                self.groups.close();
                self.groups.show(&[]);
            }
            None => {}
        }
        self.grouped.set(groups.is_some());
        self.manage.set_sensitive(subscriptions.is_some());
        match subscriptions {
            Some(rows) => self.subscriptions.show(rows),
            // The core that listed them is gone: nothing of what the dialog shows can be asked for.
            None if self.listed.get() => {
                self.subscriptions.close();
                self.subscriptions.show(&[]);
            }
            None => {}
        }
        self.listed.set(subscriptions.is_some());
        self.import.set_label(&shown.import);
        self.import.set_sensitive(shown.can_import);
        self.test.set_label(&shown.test);
        self.test.set_sensitive(shown.can_test);
        self.count.set_label(&shown.count);
        self.error
            .set_label(shown.error.as_deref().unwrap_or_default());
        self.error.set_visible(shown.error.is_some());

        self.syncing.set(true);
        {
            let mut drawn = self.drawn.borrow_mut();
            if *drawn != shown.rows {
                let item = |row: &Row| glib::BoxedAnyObject::new(row.clone());
                if drawn.len() == shown.rows.len() {
                    // The same nodes with other delays or another one in use: only those rows change, and
                    // the list stays where the user scrolled it.
                    for (place, (old, new)) in (0u32..).zip(drawn.iter().zip(&shown.rows)) {
                        if old != new {
                            self.store.splice(place, 1, &[item(new)]);
                        }
                    }
                } else {
                    let items: Vec<glib::BoxedAnyObject> = shown.rows.iter().map(item).collect();
                    self.store.splice(0, self.store.n_items(), &items);
                }
                drawn.clone_from(&shown.rows);
            }
        }
        if !switching {
            let in_use = shown
                .selected()
                .and_then(|place| u32::try_from(place).ok())
                .unwrap_or(gtk::INVALID_LIST_POSITION);
            if self.selection.selected() != in_use {
                self.selection.set_selected(in_use);
            }
        }
        self.syncing.set(false);

        match &shown.notice {
            Some(notice) => {
                // The sentence is shown as it is written, not read as markup.
                self.notice
                    .set_description(Some(glib::markup_escape_text(notice).as_str()));
                self.stack.set_visible_child_name(NOTICE);
            }
            None => self.stack.set_visible_child_name(LIST),
        }
    }
}
