//! The subscription list's widgets: a dialog over the proxies page with one row per subscription, where a
//! subscription is downloaded again, given another name or removed. What the rows say is decided in
//! [`crate::subscriptions`]; this only draws it.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::subscriptions::{Change, Row};
use crate::Language;

/// The names of the two things the dialog can hold.
const LIST: &str = "list";
const EMPTY: &str = "empty";

/// The answers of the two questions asked before a change.
const CANCEL: &str = "cancel";
const CONFIRM: &str = "confirm";

/// One subscription's row in the open dialog.
struct Line {
    /// What the row shows now; its buttons read it when they are pressed.
    row: Rc<RefCell<Row>>,
    widget: adw::ActionRow,
    update: gtk::Button,
    rename: gtk::Button,
    remove: gtk::Button,
}

/// The dialog while it is shown.
struct Open {
    dialog: adw::Dialog,
    stack: gtk::Stack,
    list: gtk::ListBox,
    lines: Vec<Line>,
}

pub struct SubscriptionList {
    language: Language,
    changed: Rc<dyn Fn(Change)>,
    /// The rows as they are known now, whether or not the dialog is shown.
    rows: RefCell<Vec<Row>>,
    open: Rc<RefCell<Option<Open>>>,
}

/// A small button with only a picture; what it does is said in its tooltip and to a screen reader.
fn picture_button(icon: &str, name: &str, says: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_widget_name(name);
    button.set_valign(gtk::Align::Center);
    button.add_css_class("flat");
    button.set_tooltip_text(Some(says));
    button.update_property(&[gtk::accessible::Property::Label(says)]);
    button
}

impl SubscriptionList {
    /// `changed` is called with what the user asks for on a subscription.
    pub fn new(language: Language, changed: impl Fn(Change) + 'static) -> Self {
        Self {
            language,
            changed: Rc::new(changed),
            rows: RefCell::new(Vec::new()),
            open: Rc::new(RefCell::new(None)),
        }
    }

    /// Draws what is known now, also into the dialog when it is shown.
    pub fn show(&self, rows: &[Row]) {
        if *self.rows.borrow() == rows {
            return;
        }
        self.rows.borrow_mut().clear();
        self.rows.borrow_mut().extend_from_slice(rows);
        if let Some(open) = self.open.borrow_mut().as_mut() {
            self.sync(open);
        }
    }

    /// Shows the dialog over the window `parent` is in, unless it is shown already.
    pub fn present(&self, parent: &impl IsA<gtk::Widget>) {
        if self.open.borrow().is_some() {
            return;
        }
        let text = |id: &str| client_strings::text(self.language, id);

        let list = gtk::ListBox::new();
        list.set_widget_name("subscriptions");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        list.set_valign(gtk::Align::Start);
        list.set_margin_top(12);
        list.set_margin_bottom(12);
        list.set_margin_start(12);
        list.set_margin_end(12);
        list.update_property(&[gtk::accessible::Property::Label(text(
            "proxies.subscriptions",
        ))]);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&list));
        let empty = adw::StatusPage::new();
        empty.set_icon_name(Some("network-server-symbolic"));
        empty.set_description(Some(
            glib::markup_escape_text(text("subscriptions.empty")).as_str(),
        ));
        let stack = gtk::Stack::new();
        stack.add_named(&scroller, Some(LIST));
        stack.add_named(&empty, Some(EMPTY));

        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&stack));
        let dialog = adw::Dialog::new();
        dialog.set_title(text("proxies.subscriptions"));
        dialog.set_content_width(560);
        dialog.set_content_height(480);
        dialog.set_child(Some(&view));
        dialog.connect_closed({
            let open = Rc::clone(&self.open);
            move |_| {
                open.borrow_mut().take();
            }
        });

        let mut open = Open {
            dialog: dialog.clone(),
            stack,
            list,
            lines: Vec::new(),
        };
        self.sync(&mut open);
        *self.open.borrow_mut() = Some(open);
        dialog.present(Some(parent));
    }

    /// Closes the dialog if it is shown: what it lists belongs to a core that is gone.
    pub fn close(&self) {
        let dialog = self.open.borrow().as_ref().map(|open| open.dialog.clone());
        if let Some(dialog) = dialog {
            dialog.close();
        }
    }

    /// Makes the open dialog show [`Self::rows`]. The same subscriptions in the same order keep their
    /// rows, so that the keyboard focus stays on the button that was pressed.
    fn sync(&self, open: &mut Open) {
        let rows = self.rows.borrow();
        let same = open.lines.len() == rows.len()
            && open
                .lines
                .iter()
                .zip(rows.iter())
                .all(|(line, row)| line.row.borrow().id == row.id);
        if !same {
            open.list.remove_all();
            open.lines = rows.iter().map(|row| self.line(row)).collect();
            for line in &open.lines {
                open.list.append(&line.widget);
            }
        }
        for (line, row) in open.lines.iter().zip(rows.iter()) {
            line.widget.set_title(&row.title);
            let below = match &row.error {
                Some(error) if row.details.is_empty() => error.clone(),
                Some(error) => format!("{}\n{error}", row.details),
                None => row.details.clone(),
            };
            line.widget.set_subtitle(&below);
            line.update.set_visible(row.update.is_some());
            line.update
                .set_label(row.update.as_deref().unwrap_or_default());
            line.update.set_sensitive(row.enabled);
            line.rename.set_sensitive(row.enabled);
            line.remove.set_sensitive(row.enabled);
            *line.row.borrow_mut() = row.clone();
        }
        open.stack
            .set_visible_child_name(if rows.is_empty() { EMPTY } else { LIST });
    }

    /// Makes the widgets of one subscription's row.
    fn line(&self, row: &Row) -> Line {
        let language = self.language;
        let text = move |id: &str| client_strings::text(language, id);
        let row = Rc::new(RefCell::new(row.clone()));

        let widget = adw::ActionRow::new();
        // Names and the core's sentences are shown as they are written, not read as markup.
        widget.set_use_markup(false);
        widget.set_subtitle_lines(0);

        let update = gtk::Button::new();
        update.set_widget_name("update");
        update.set_valign(gtk::Align::Center);
        update.connect_clicked({
            let row = Rc::clone(&row);
            let changed = Rc::clone(&self.changed);
            move |_| changed(Change::Update(row.borrow().id.clone()))
        });

        let rename = picture_button(
            "document-edit-symbolic",
            "rename",
            text("subscriptions.rename"),
        );
        rename.connect_clicked({
            let row = Rc::clone(&row);
            let changed = Rc::clone(&self.changed);
            move |button| {
                let (id, name) = {
                    let row = row.borrow();
                    (row.id.clone(), row.name.clone())
                };
                let entry = gtk::Entry::new();
                entry.set_widget_name("name");
                entry.set_text(&name);
                entry.set_activates_default(true);
                entry.update_property(&[gtk::accessible::Property::Label(text(
                    "subscriptions.name",
                ))]);
                let question = adw::AlertDialog::new(Some(text("subscriptions.rename")), None);
                question.set_extra_child(Some(&entry));
                question.add_response(CANCEL, text("action.cancel"));
                question.add_response(CONFIRM, text("subscriptions.save"));
                question.set_response_appearance(CONFIRM, adw::ResponseAppearance::Suggested);
                question.set_default_response(Some(CONFIRM));
                question.set_close_response(CANCEL);
                question.connect_response(None, {
                    let changed = Rc::clone(&changed);
                    move |_, response| {
                        let name = entry.text().trim().to_owned();
                        // A name of nothing is no name; the core would say so too.
                        if response == CONFIRM && !name.is_empty() {
                            changed(Change::Rename(id.clone(), name));
                        }
                    }
                });
                question.present(Some(button));
            }
        });

        let remove = picture_button(
            "user-trash-symbolic",
            "remove",
            text("subscriptions.remove"),
        );
        remove.connect_clicked({
            let row = Rc::clone(&row);
            let changed = Rc::clone(&self.changed);
            move |button| {
                let (id, title) = {
                    let row = row.borrow();
                    (row.id.clone(), row.title.clone())
                };
                let heading = client_strings::format(
                    language,
                    "subscriptions.remove.confirm",
                    &[title.as_str()],
                );
                let question = adw::AlertDialog::new(Some(&heading), None);
                question.add_response(CANCEL, text("action.cancel"));
                question.add_response(CONFIRM, text("subscriptions.remove"));
                question.set_response_appearance(CONFIRM, adw::ResponseAppearance::Destructive);
                // Enter does not remove anything: the answer that is ready is the harmless one.
                question.set_default_response(Some(CANCEL));
                question.set_close_response(CANCEL);
                question.connect_response(None, {
                    let changed = Rc::clone(&changed);
                    move |_, response| {
                        if response == CONFIRM {
                            changed(Change::Remove(id.clone()));
                        }
                    }
                });
                question.present(Some(button));
            }
        });

        widget.add_suffix(&update);
        widget.add_suffix(&rename);
        widget.add_suffix(&remove);
        Line {
            row,
            widget,
            update,
            rename,
            remove,
        }
    }
}
