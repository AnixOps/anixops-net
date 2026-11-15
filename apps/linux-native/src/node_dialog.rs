//! One node's dialog: its name, server and port, which can be changed for a node of the subscription in
//! use, the button that removes it, and its share link with a QR code. What it says is decided in
//! [`crate::node`]; this only draws it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::node::{self, Draft, Shown};
use crate::Language;

/// The answers of the question asked before a removal.
const CANCEL: &str = "cancel";
const CONFIRM: &str = "confirm";

/// How many pixels a module of the QR code is wide in its picture; the picture is scaled to its place.
const MODULE: usize = 6;
/// The side of the QR code on screen, in logical pixels.
const QR_SIDE: i32 = 220;

/// What the user asked for in the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wish {
    /// Save what was typed.
    Save(Draft),
    /// Remove the node.
    Remove,
    /// The dialog was closed.
    Closed,
}

/// The dialog while it is shown.
struct Open {
    dialog: adw::Dialog,
    protocol: adw::PreferencesGroup,
    name: adw::EntryRow,
    server: adw::EntryRow,
    port: adw::EntryRow,
    problem: gtk::Label,
    save: gtk::Button,
    remove: gtk::Button,
    keep: gtk::Label,
    share: adw::PreferencesGroup,
    qr: gtk::Picture,
    link: gtk::Label,
    copy: gtk::Button,
    /// What the dialog shows now, and whether that was drawn yet.
    shown: Rc<RefCell<Shown>>,
    drawn: Cell<bool>,
    /// Set while the dialog is closed because the node is no longer shown, not by the user.
    leaving: Rc<Cell<bool>>,
}

pub struct NodeDialog {
    language: Language,
    wished: Rc<dyn Fn(Wish)>,
    open: Rc<RefCell<Option<Open>>>,
}

impl NodeDialog {
    /// `wished` is called with what the user asks for in the dialog.
    pub fn new(language: Language, wished: impl Fn(Wish) + 'static) -> Self {
        Self {
            language,
            wished: Rc::new(wished),
            open: Rc::new(RefCell::new(None)),
        }
    }

    /// Draws what is known now: the dialog of the node `shown` is about over the window `parent` is in, or
    /// no dialog for `None`.
    pub fn show(&self, parent: &impl IsA<gtk::Widget>, shown: Option<&Shown>) {
        let Some(shown) = shown else {
            let open = self.open.borrow_mut().take();
            if let Some(open) = open {
                open.leaving.set(true);
                open.dialog.close();
            }
            return;
        };
        if self.open.borrow().is_none() {
            let open = self.build(shown);
            open.dialog.present(Some(parent));
            *self.open.borrow_mut() = Some(open);
        }
        if let Some(open) = self.open.borrow().as_ref() {
            self.draw(open, shown);
        }
    }

    /// Writes `shown` into the open dialog. The boxes are only written when the node behind them changed:
    /// what the user is typing is not taken away by an answer about something else.
    fn draw(&self, open: &Open, shown: &Shown) {
        let before = open.shown.borrow().clone();
        let first = !open.drawn.replace(true) || before.id != shown.id;
        if !first && before == *shown {
            return;
        }
        if first || before.draft != shown.draft {
            open.name.set_text(&shown.draft.name);
            open.server.set_text(&shown.draft.server);
            open.port.set_text(&shown.draft.port);
        }
        open.dialog.set_title(&shown.title);
        open.protocol
            .set_title(&glib::markup_escape_text(&shown.protocol));
        open.protocol
            .set_description(Some(glib::markup_escape_text(&shown.note).as_str()));
        for row in [&open.name, &open.server, &open.port] {
            row.set_editable(shown.editable);
        }
        open.problem
            .set_label(shown.problem.as_deref().unwrap_or_default());
        open.problem.set_visible(shown.problem.is_some());
        open.save.set_visible(shown.editable);
        open.save.set_sensitive(shown.can_save);
        open.remove.set_visible(shown.editable);
        open.remove.set_sensitive(shown.can_remove);
        open.keep
            .set_label(shown.keep.as_deref().unwrap_or_default());
        open.keep.set_visible(shown.keep.is_some());

        let text = |id: &str| client_strings::text(self.language, id);
        match &shown.link {
            Some(link) => {
                open.share.set_description(None);
                open.link.set_label(link);
            }
            None => {
                open.share.set_description(Some(
                    glib::markup_escape_text(text("node.share.none")).as_str(),
                ));
                open.link.set_label("");
            }
        }
        open.link.set_visible(shown.link.is_some());
        open.copy.set_visible(shown.link.is_some());
        if first || before.link != shown.link {
            open.copy.set_label(text("node.copy"));
        }
        if first || before.qr != shown.qr {
            match &shown.qr {
                Some(rows) => {
                    let (side, pixels) = node::picture(rows, MODULE);
                    let texture = gdk::MemoryTexture::new(
                        side as i32,
                        side as i32,
                        gdk::MemoryFormat::R8g8b8,
                        &glib::Bytes::from_owned(pixels),
                        side * 3,
                    );
                    open.qr.set_paintable(Some(&texture));
                }
                None => open.qr.set_paintable(gdk::Paintable::NONE),
            }
        }
        open.qr.set_visible(shown.qr.is_some());
        *open.shown.borrow_mut() = shown.clone();
    }

    /// Makes the dialog's widgets.
    fn build(&self, shown: &Shown) -> Open {
        let language = self.language;
        let text = move |id: &str| client_strings::text(language, id);
        let shown = Rc::new(RefCell::new(shown.clone()));

        let entry = |name: &str, title: &str| {
            let row = adw::EntryRow::new();
            row.set_widget_name(name);
            row.set_title(title);
            row
        };
        let name = entry("name", text("node.name"));
        let server = entry("server", text("node.server"));
        server.set_input_purpose(gtk::InputPurpose::Url);
        let port = entry("port", text("node.port"));
        port.set_input_purpose(gtk::InputPurpose::Digits);
        let protocol = adw::PreferencesGroup::new();
        protocol.add(&name);
        protocol.add(&server);
        protocol.add(&port);

        let problem = gtk::Label::new(None);
        problem.set_widget_name("problem");
        problem.set_xalign(0.0);
        problem.set_wrap(true);
        problem.add_css_class("error");
        // So that the reason can be copied into a message asking for help.
        problem.set_selectable(true);
        let keep = gtk::Label::new(None);
        keep.set_xalign(0.0);
        keep.set_wrap(true);
        keep.add_css_class("dim-label");
        let save = gtk::Button::with_label(text("action.save"));
        save.set_widget_name("save");
        save.add_css_class("suggested-action");
        let remove = gtk::Button::with_label(text("node.remove"));
        remove.set_widget_name("remove");
        remove.add_css_class("destructive-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&remove);
        buttons.append(&save);
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
        actions.append(&problem);
        actions.append(&keep);
        actions.append(&buttons);
        let acting = adw::PreferencesGroup::new();
        acting.add(&actions);

        let wish_save = {
            let (name, server, port) = (name.clone(), server.clone(), port.clone());
            let shown = Rc::clone(&shown);
            let wished = Rc::clone(&self.wished);
            move || {
                if shown.borrow().can_save {
                    wished(Wish::Save(Draft {
                        name: name.text().to_string(),
                        server: server.text().to_string(),
                        port: port.text().to_string(),
                    }));
                }
            }
        };
        save.connect_clicked({
            let wish_save = wish_save.clone();
            move |_| wish_save()
        });
        // Enter in a box saves, as the button does.
        for row in [&name, &server, &port] {
            row.connect_entry_activated({
                let wish_save = wish_save.clone();
                move |_| wish_save()
            });
        }
        remove.connect_clicked({
            let shown = Rc::clone(&shown);
            let wished = Rc::clone(&self.wished);
            move |button| {
                let title = {
                    let shown = shown.borrow();
                    if !shown.can_remove {
                        return;
                    }
                    shown.title.clone()
                };
                let heading =
                    client_strings::format(language, "node.remove.confirm", &[title.as_str()]);
                let question = adw::AlertDialog::new(Some(&heading), Some(text("node.edit.note")));
                question.add_response(CANCEL, text("action.cancel"));
                question.add_response(CONFIRM, text("subscriptions.remove"));
                question.set_response_appearance(CONFIRM, adw::ResponseAppearance::Destructive);
                // Enter does not remove anything: the answer that is ready is the harmless one.
                question.set_default_response(Some(CANCEL));
                question.set_close_response(CANCEL);
                question.connect_response(None, {
                    let wished = Rc::clone(&wished);
                    move |_, response| {
                        if response == CONFIRM {
                            wished(Wish::Remove);
                        }
                    }
                });
                question.present(Some(button));
            }
        });

        let qr = gtk::Picture::new();
        qr.set_widget_name("qr");
        qr.set_size_request(QR_SIDE, QR_SIDE);
        qr.set_halign(gtk::Align::Center);
        qr.set_can_shrink(true);
        qr.set_content_fit(gtk::ContentFit::Contain);
        // A picture's alternative text is only its description to a screen reader; this is its name.
        qr.update_property(&[gtk::accessible::Property::Label(text("node.qr"))]);
        let link = gtk::Label::new(None);
        link.set_widget_name("link");
        link.set_xalign(0.0);
        link.set_wrap(true);
        link.set_wrap_mode(gtk::pango::WrapMode::Char);
        link.set_selectable(true);
        link.add_css_class("caption");
        link.add_css_class("monospace");
        let copy = gtk::Button::with_label(text("node.copy"));
        copy.set_widget_name("copy");
        copy.set_halign(gtk::Align::End);
        copy.connect_clicked({
            let shown = Rc::clone(&shown);
            move |button| {
                if let Some(link) = shown.borrow().link.as_deref() {
                    button.clipboard().set_text(link);
                    button.set_label(text("node.copied"));
                }
            }
        });
        let sharing = gtk::Box::new(gtk::Orientation::Vertical, 12);
        sharing.append(&qr);
        sharing.append(&link);
        sharing.append(&copy);
        let share = adw::PreferencesGroup::new();
        share.set_title(text("node.share"));
        share.add(&sharing);

        let page = adw::PreferencesPage::new();
        page.add(&protocol);
        page.add(&acting);
        page.add(&share);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&page));
        let dialog = adw::Dialog::new();
        dialog.set_content_width(480);
        dialog.set_content_height(640);
        dialog.set_child(Some(&view));
        let leaving = Rc::new(Cell::new(false));
        dialog.connect_closed({
            let open = Rc::clone(&self.open);
            let leaving = Rc::clone(&leaving);
            let wished = Rc::clone(&self.wished);
            move |_| {
                if !leaving.get() {
                    open.borrow_mut().take();
                    wished(Wish::Closed);
                }
            }
        });

        Open {
            dialog,
            protocol,
            name,
            server,
            port,
            problem,
            save,
            remove,
            keep,
            share,
            qr,
            link,
            copy,
            shown,
            drawn: Cell::new(false),
            leaving,
        }
    }
}
