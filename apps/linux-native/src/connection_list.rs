//! The open connections' widgets: a dialog over the overview page with a search, the count, "close all" and
//! one row per connection with the button that closes it. What the rows say is decided in
//! [`crate::connections`]; this only draws it.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::connections::{Row, Shown};
use crate::Language;

/// The names of the two things the dialog can hold under its search.
const LIST: &str = "list";
const NOTICE: &str = "notice";

/// What the user asked for in the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wish {
    /// The dialog was opened: what it lists is to be asked for.
    Opened,
    /// The search holds this now.
    Search(String),
    /// Close the connection with this id.
    Close(u64),
    /// Close every open connection.
    CloseAll,
    /// The dialog was closed.
    Closed,
}

/// One connection's row in the open dialog.
struct Line {
    id: u64,
    widget: adw::ActionRow,
    close: gtk::Button,
}

/// The dialog while it is shown.
struct Open {
    dialog: adw::Dialog,
    search: gtk::SearchEntry,
    count: gtk::Label,
    close_all: gtk::Button,
    stack: gtk::Stack,
    notice: adw::StatusPage,
    list: gtk::ListBox,
    lines: Vec<Line>,
}

pub struct ConnectionList {
    language: Language,
    wished: Rc<dyn Fn(Wish)>,
    /// The list as it is known now, whether or not the dialog is shown.
    shown: RefCell<Option<Shown>>,
    open: Rc<RefCell<Option<Open>>>,
}

impl ConnectionList {
    /// `wished` is called with what the user asks for in the dialog.
    pub fn new(language: Language, wished: impl Fn(Wish) + 'static) -> Self {
        Self {
            language,
            wished: Rc::new(wished),
            shown: RefCell::new(None),
            open: Rc::new(RefCell::new(None)),
        }
    }

    /// Draws what is known now, also into the dialog when it is shown. `None` while the core has not said.
    pub fn show(&self, shown: Option<&Shown>) {
        if self.shown.borrow().as_ref() == shown {
            return;
        }
        *self.shown.borrow_mut() = shown.cloned();
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

        let search = gtk::SearchEntry::new();
        search.set_widget_name("search");
        search.set_hexpand(true);
        search.set_placeholder_text(Some(text("connections.search")));
        search.update_property(&[gtk::accessible::Property::Label(text("connections.search"))]);
        search.connect_search_changed({
            let wished = Rc::clone(&self.wished);
            move |search| wished(Wish::Search(search.text().to_string()))
        });
        let close_all = gtk::Button::with_label(text("connections.close_all"));
        close_all.set_widget_name("close-all");
        close_all.connect_clicked({
            let wished = Rc::clone(&self.wished);
            move |_| wished(Wish::CloseAll)
        });
        let tools = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        tools.append(&search);
        tools.append(&close_all);
        let count = gtk::Label::new(None);
        count.set_widget_name("count");
        count.set_xalign(0.0);
        count.add_css_class("dim-label");
        count.add_css_class("caption");

        let list = gtk::ListBox::new();
        list.set_widget_name("connections");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        list.set_valign(gtk::Align::Start);
        list.update_property(&[gtk::accessible::Property::Label(text(
            "overview.connections",
        ))]);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&list));
        let notice = adw::StatusPage::new();
        notice.set_icon_name(Some("network-transmit-receive-symbolic"));
        notice.set_vexpand(true);
        let stack = gtk::Stack::new();
        stack.add_named(&scroller, Some(LIST));
        stack.add_named(&notice, Some(NOTICE));

        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&tools);
        content.append(&count);
        content.append(&stack);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&content));
        let dialog = adw::Dialog::new();
        dialog.set_title(text("overview.connections"));
        dialog.set_content_width(640);
        dialog.set_content_height(520);
        dialog.set_child(Some(&view));
        dialog.connect_closed({
            let open = Rc::clone(&self.open);
            let wished = Rc::clone(&self.wished);
            move |_| {
                open.borrow_mut().take();
                wished(Wish::Closed);
            }
        });

        let mut open = Open {
            dialog: dialog.clone(),
            search,
            count,
            close_all,
            stack,
            notice,
            list,
            lines: Vec::new(),
        };
        self.sync(&mut open);
        *self.open.borrow_mut() = Some(open);
        dialog.present(Some(parent));
        (self.wished)(Wish::Opened);
    }

    /// Makes the open dialog show [`Self::shown`]. The same connections in the same order keep their rows,
    /// so that the keyboard focus stays where it was while the numbers change.
    fn sync(&self, open: &mut Open) {
        let shown = self.shown.borrow();
        let Some(shown) = shown.as_ref() else {
            // The core has not said yet: nothing to search or close, and no sentence about it either.
            open.count.set_label("");
            open.search.set_sensitive(false);
            open.close_all.set_sensitive(false);
            open.notice.set_description(None);
            open.stack.set_visible_child_name(NOTICE);
            return;
        };
        let rows = &shown.rows;
        let same = open.lines.len() == rows.len()
            && open
                .lines
                .iter()
                .zip(rows.iter())
                .all(|(line, row)| line.id == row.id);
        if !same {
            open.list.remove_all();
            let lines = rows.iter().map(|row| self.line(row)).collect();
            open.lines = lines;
            for line in &open.lines {
                open.list.append(&line.widget);
            }
        }
        for (line, row) in open.lines.iter().zip(rows.iter()) {
            line.widget.set_title(&row.title);
            line.widget.set_subtitle(&row.details);
            line.close.set_sensitive(row.enabled);
        }
        open.count.set_label(&shown.count);
        // The search stays usable while it leaves nothing, so that it can be changed.
        open.search
            .set_sensitive(shown.can_search || !open.search.text().is_empty());
        open.close_all.set_sensitive(shown.can_close_all);
        match &shown.notice {
            Some(notice) => {
                open.notice
                    .set_description(Some(glib::markup_escape_text(notice).as_str()));
                open.stack.set_visible_child_name(NOTICE);
            }
            None => open.stack.set_visible_child_name(LIST),
        }
    }

    /// Makes one connection's row, with the button that closes it.
    fn line(&self, row: &Row) -> Line {
        let says = client_strings::text(self.language, "connections.close");
        let widget = adw::ActionRow::new();
        // Hosts and rules are shown as they are written, not read as markup.
        widget.set_use_markup(false);
        widget.set_subtitle_lines(0);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_widget_name("close");
        close.set_valign(gtk::Align::Center);
        close.add_css_class("flat");
        // The button shows only a picture: what it does is said in its tooltip and to a screen reader.
        close.set_tooltip_text(Some(says));
        close.update_property(&[gtk::accessible::Property::Label(says)]);
        close.connect_clicked({
            let wished = Rc::clone(&self.wished);
            let id = row.id;
            move |_| wished(Wish::Close(id))
        });
        widget.add_suffix(&close);
        Line {
            id: row.id,
            widget,
            close,
        }
    }
}
