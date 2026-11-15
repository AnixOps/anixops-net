//! The policy group list's widgets: a dialog over the proxies page with one row per group. A group the user
//! chooses in opens a second page of the dialog with its members, where the selected one is the member in
//! use, as the selected row of the proxies page is the node in use. What the rows say is decided in
//! [`crate::groups`]; this only draws it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::groups::{Choice, Row};
use crate::Language;

/// The names of the two things the dialog's first page can hold.
const LIST: &str = "list";
const EMPTY: &str = "empty";

/// One group's row in the open dialog.
struct Line {
    /// What the row shows now; opening its members reads it.
    row: Rc<RefCell<Row>>,
    widget: adw::ActionRow,
}

/// The page of one group's members while it is shown.
struct Members {
    /// The group as it is known now; choosing a member reads it.
    row: Rc<RefCell<Row>>,
    selection: gtk::SingleSelection,
    /// Set while the dialog itself moves the selection, so that this is not taken for the user's choice.
    drawing: Rc<Cell<bool>>,
}

/// The dialog while it is shown.
struct Open {
    dialog: adw::Dialog,
    pages: adw::NavigationView,
    stack: gtk::Stack,
    list: gtk::ListBox,
    lines: Vec<Line>,
    members: Rc<RefCell<Option<Members>>>,
}

pub struct GroupList {
    language: Language,
    chosen: Rc<dyn Fn(Choice)>,
    /// The rows as they are known now, whether or not the dialog is shown.
    rows: RefCell<Vec<Row>>,
    open: Rc<RefCell<Option<Open>>>,
}

/// Whether two rows are the same group offering the same members, so that one's widgets can show the other.
fn same_shape(one: &Row, other: &Row) -> bool {
    one.tag == other.tag && one.options == other.options
}

/// The place of a row's member in use in a list, or the place that means "none".
fn place(row: &Row) -> u32 {
    row.chosen
        .and_then(|at| u32::try_from(at).ok())
        .unwrap_or(gtk::INVALID_LIST_POSITION)
}

/// Shows the members of `row`'s group as the dialog's second page.
fn show_members(
    pages: &adw::NavigationView,
    members: &Rc<RefCell<Option<Members>>>,
    row: &Row,
    chosen: &Rc<dyn Fn(Choice)>,
) {
    let row = Rc::new(RefCell::new(row.clone()));
    let drawing = Rc::new(Cell::new(false));

    let names = gtk::StringList::new(&[]);
    for member in &row.borrow().options {
        names.append(&member.label);
    }
    let selection = gtk::SingleSelection::new(Some(names));
    // No member is chosen for the user: the selected one is the member the group uses, or none.
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    selection.set_selected(place(&row.borrow()));
    selection.connect_selected_notify({
        let row = Rc::clone(&row);
        let drawing = Rc::clone(&drawing);
        let chosen = Rc::clone(chosen);
        move |selection| {
            if drawing.get() {
                return;
            }
            let choice = {
                let row = row.borrow();
                let at = usize::try_from(selection.selected()).ok();
                // Choosing the member that is in use asks for nothing.
                if at == row.chosen {
                    return;
                }
                at.and_then(|at| row.options.get(at)).map(|member| Choice {
                    group: row.tag.clone(),
                    member: member.id.clone(),
                })
            };
            if let Some(choice) = choice {
                chosen(choice);
            }
        }
    });

    let names = gtk::SignalListItemFactory::new();
    names.connect_setup(|_, object| {
        if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            item.set_child(Some(&label));
        }
    });
    names.connect_bind(|_, object| {
        let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let (Some(label), Some(name)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) {
            // Names are shown as they are written, not read as markup.
            label.set_label(name.string().as_str());
        }
    });
    let list = gtk::ListView::new(Some(selection.clone()), Some(names));
    list.set_widget_name("members");
    list.add_css_class("rich-list");
    list.update_property(&[gtk::accessible::Property::Label(row.borrow().tag.as_str())]);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_child(Some(&list));

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&scroller));
    let page = adw::NavigationPage::new(&view, row.borrow().tag.as_str());
    page.connect_hidden({
        let members = Rc::clone(members);
        let row = Rc::clone(&row);
        move |_| {
            // Only the page that is going: another group's page may have been shown since.
            let mut members = members.borrow_mut();
            if members
                .as_ref()
                .is_some_and(|shown| Rc::ptr_eq(&shown.row, &row))
            {
                members.take();
            }
        }
    });
    *members.borrow_mut() = Some(Members {
        row,
        selection,
        drawing,
    });
    pages.push(&page);
}

impl GroupList {
    /// `chosen` is called with the member the user chooses for a group.
    pub fn new(language: Language, chosen: impl Fn(Choice) + 'static) -> Self {
        Self {
            language,
            chosen: Rc::new(chosen),
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
        list.set_widget_name("groups");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        list.set_valign(gtk::Align::Start);
        list.set_margin_top(12);
        list.set_margin_bottom(12);
        list.set_margin_start(12);
        list.set_margin_end(12);
        list.update_property(&[gtk::accessible::Property::Label(text("proxies.groups"))]);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&list));
        let empty = adw::StatusPage::new();
        empty.set_icon_name(Some("network-workgroup-symbolic"));
        empty.set_description(Some(
            glib::markup_escape_text(text("groups.empty")).as_str(),
        ));
        let stack = gtk::Stack::new();
        stack.add_named(&scroller, Some(LIST));
        stack.add_named(&empty, Some(EMPTY));

        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&stack));
        let pages = adw::NavigationView::new();
        pages.add(&adw::NavigationPage::new(&view, text("proxies.groups")));
        let dialog = adw::Dialog::new();
        dialog.set_title(text("proxies.groups"));
        dialog.set_content_width(560);
        dialog.set_content_height(480);
        dialog.set_child(Some(&pages));
        dialog.connect_closed({
            let open = Rc::clone(&self.open);
            move |_| {
                open.borrow_mut().take();
            }
        });

        let mut open = Open {
            dialog: dialog.clone(),
            pages,
            stack,
            list,
            lines: Vec::new(),
            members: Rc::new(RefCell::new(None)),
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

    /// Makes the open dialog show [`Self::rows`]. The same groups with the same members keep their rows,
    /// so that the keyboard focus stays where it was.
    fn sync(&self, open: &mut Open) {
        let rows = self.rows.borrow();
        let same = open.lines.len() == rows.len()
            && open
                .lines
                .iter()
                .zip(rows.iter())
                .all(|(line, row)| same_shape(&line.row.borrow(), row));
        if !same {
            open.list.remove_all();
            let lines = rows.iter().map(|row| self.line(open, row)).collect();
            open.lines = lines;
            for line in &open.lines {
                open.list.append(&line.widget);
            }
        }
        for (line, row) in open.lines.iter().zip(rows.iter()) {
            *line.row.borrow_mut() = row.clone();
            line.widget.set_title(&row.tag);
            line.widget.set_subtitle(&row.details);
        }
        open.stack
            .set_visible_child_name(if rows.is_empty() { EMPTY } else { LIST });

        // The page of members that is shown follows its group, and goes when the group is no longer the
        // one it was opened for.
        let stays = {
            let members = open.members.borrow();
            let Some(members) = members.as_ref() else {
                return;
            };
            let now = rows
                .iter()
                .find(|row| same_shape(&members.row.borrow(), row));
            if let Some(row) = now {
                *members.row.borrow_mut() = row.clone();
                // While the core is changing the group the member the user chose stays selected,
                // whatever the core still says is in use.
                if row.enabled && members.selection.selected() != place(row) {
                    members.drawing.set(true);
                    members.selection.set_selected(place(row));
                    members.drawing.set(false);
                }
            }
            now.is_some()
        };
        if !stays {
            open.pages.pop();
        }
    }

    /// Makes one group's row. One the user chooses in opens the group's members when it is pressed.
    fn line(&self, open: &Open, row: &Row) -> Line {
        let widget = adw::ActionRow::new();
        // Names are shown as they are written, not read as markup.
        widget.set_use_markup(false);
        widget.set_subtitle_lines(0);
        let row = Rc::new(RefCell::new(row.clone()));
        if !row.borrow().options.is_empty() {
            let says = client_strings::text(self.language, "groups.choose");
            let choose = gtk::Button::from_icon_name("go-next-symbolic");
            choose.set_widget_name("choose");
            choose.set_valign(gtk::Align::Center);
            choose.add_css_class("flat");
            // To a screen reader the button is called by the group's name: the row names the widget that
            // pressing it activates after its title. What it does is in its tooltip.
            choose.set_tooltip_text(Some(says));
            choose.connect_clicked({
                let pages = open.pages.clone();
                let members = Rc::clone(&open.members);
                let row = Rc::clone(&row);
                let chosen = Rc::clone(&self.chosen);
                move |_| show_members(&pages, &members, &row.borrow(), &chosen)
            });
            widget.add_suffix(&choose);
            widget.set_activatable_widget(Some(&choose));
        }
        Line { row, widget }
    }
}
