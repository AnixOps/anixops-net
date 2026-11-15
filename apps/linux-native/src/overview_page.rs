//! The overview page's widgets: the state in large letters, the button that connects and disconnects, the
//! node in use and the traffic counters. The row that counts the open connections opens their list
//! ([`crate::connection_list`]). What they say is decided in [`crate::overview`]; this only draws it.

use adw::prelude::*;

use crate::connection_list::{ConnectionList, Wish};
use crate::connections;
use crate::overview::{Press, Shown};
use crate::Language;

/// The widest the page's column gets, in logical pixels.
const COLUMN: i32 = 640;

pub struct OverviewPage {
    root: gtk::ScrolledWindow,
    headline: gtk::Label,
    detail: gtk::Label,
    error: gtk::Label,
    button: gtk::Button,
    node: adw::ActionRow,
    download: adw::ActionRow,
    download_rate: gtk::Label,
    upload: adw::ActionRow,
    upload_rate: gtk::Label,
    connections: gtk::Label,
    /// The list of open connections, a dialog over the page.
    list: std::rc::Rc<ConnectionList>,
}

/// A line of text that wraps and sits in the middle.
fn centred(classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_wrap(true);
    label.set_justify(gtk::Justification::Center);
    for class in classes {
        label.add_css_class(class);
    }
    label
}

/// A row that names something on the left; its value goes on the right or underneath.
fn row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    // Names of nodes are shown as they are written, not read as markup.
    row.set_use_markup(false);
    row.set_title(title);
    row
}

/// A number at the right end of a row.
fn value(row: &adw::ActionRow) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("numeric");
    row.add_suffix(&label);
    label
}

impl OverviewPage {
    /// Builds the page. `pressed` is called when the user presses the button, `wished` with what the user
    /// asks for in the list of open connections.
    pub fn new(
        language: Language,
        pressed: impl Fn() + 'static,
        wished: impl Fn(Wish) + 'static,
    ) -> Self {
        let text = |id: &str| client_strings::text(language, id);

        let headline = centred(&["title-1"]);
        let detail = centred(&["dim-label"]);
        let error = centred(&["error"]);
        // So that the reason can be copied into a message asking for help.
        error.set_selectable(true);
        let button = gtk::Button::new();
        button.set_widget_name("connect");
        button.add_css_class("pill");
        button.set_halign(gtk::Align::Center);
        button.connect_clicked(move |_| pressed());
        let top = gtk::Box::new(gtk::Orientation::Vertical, 12);
        top.set_margin_top(24);
        top.append(&headline);
        top.append(&detail);
        top.append(&button);
        top.append(&error);

        let node = row(text("overview.node"));
        // The value under the name, in the larger letters.
        node.add_css_class("property");
        let node_group = adw::PreferencesGroup::new();
        node_group.add(&node);

        let download = row(text("overview.download"));
        let download_rate = value(&download);
        let upload = row(text("overview.upload"));
        let upload_rate = value(&upload);
        let connection_row = row(text("overview.connections"));
        let connections = value(&connection_row);
        let list = std::rc::Rc::new(ConnectionList::new(language, wished));
        // The row opens the list of the connections it counts. To a screen reader the button is called by
        // the row's title; what it does is in its tooltip.
        let open = gtk::Button::from_icon_name("go-next-symbolic");
        open.set_widget_name("connections");
        open.set_valign(gtk::Align::Center);
        open.add_css_class("flat");
        open.set_tooltip_text(Some(text("overview.connections.open")));
        open.connect_clicked({
            let list = std::rc::Rc::clone(&list);
            move |button| list.present(button)
        });
        connection_row.add_suffix(&open);
        connection_row.set_activatable_widget(Some(&open));
        let traffic_group = adw::PreferencesGroup::new();
        traffic_group.set_title(text("overview.traffic"));
        traffic_group.add(&download);
        traffic_group.add(&upload);
        traffic_group.add(&connection_row);

        let column = gtk::Box::new(gtk::Orientation::Vertical, 24);
        column.set_margin_top(12);
        column.set_margin_bottom(24);
        column.set_margin_start(16);
        column.set_margin_end(16);
        column.append(&top);
        column.append(&node_group);
        column.append(&traffic_group);
        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(COLUMN);
        clamp.set_child(Some(&column));
        let root = gtk::ScrolledWindow::new();
        root.set_hscrollbar_policy(gtk::PolicyType::Never);
        root.set_child(Some(&clamp));

        Self {
            root,
            headline,
            detail,
            error,
            button,
            node,
            download,
            download_rate,
            upload,
            upload_rate,
            connections,
            list,
        }
    }

    /// The page, to put into the window.
    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// Opens the list of open connections over the window the page is in.
    pub fn open_connections(&self) {
        self.list.present(&self.root);
    }

    /// Draws what is known now: the page, and the list of open connections while it is shown.
    pub fn show(&self, shown: &Shown, connections: Option<&connections::Shown>) {
        self.headline.set_label(&shown.headline);
        for (label, text) in [(&self.detail, &shown.detail), (&self.error, &shown.error)] {
            label.set_label(text.as_deref().unwrap_or_default());
            label.set_visible(text.is_some());
        }
        self.button.set_label(&shown.button);
        self.button.set_sensitive(shown.enabled);
        // Connecting is the thing to do on this page; disconnecting is offered, not urged.
        if shown.press == Press::Connect {
            self.button.add_css_class("suggested-action");
        } else {
            self.button.remove_css_class("suggested-action");
        }
        self.node.set_subtitle(&shown.node);
        self.download_rate.set_label(&shown.download_rate);
        self.download
            .set_subtitle(shown.download_total.as_deref().unwrap_or_default());
        self.upload_rate.set_label(&shown.upload_rate);
        self.upload
            .set_subtitle(shown.upload_total.as_deref().unwrap_or_default());
        self.connections.set_label(&shown.connections);
        self.list.show(connections);
    }
}
