//! Browsing a list with the keyboard without choosing what is passed over.
//!
//! In the proxies list and a policy group's members the selected row is the node or member in use, and selecting
//! another row asks the core to use it. GTK selects the focused row as the arrow keys (and Home, End, Page Up,
//! Page Down) move the focus, so browsing would switch nodes one by one. [`mark_browsing`] marks those key presses: the
//! list's selection handler then puts the selection back on the row in use instead of asking for anything, and the
//! focus moves on alone. A click, Space or Enter on a row still chooses it.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

/// The keys that move the focus through a list.
const BROWSING: [gdk::Key; 12] = [
    gdk::Key::Up,
    gdk::Key::Down,
    gdk::Key::Home,
    gdk::Key::End,
    gdk::Key::Page_Up,
    gdk::Key::Page_Down,
    gdk::Key::KP_Up,
    gdk::Key::KP_Down,
    gdk::Key::KP_Home,
    gdk::Key::KP_End,
    gdk::Key::KP_Page_Up,
    gdk::Key::KP_Page_Down,
];

/// Sets `browsing` while `list` handles a browsing key, so the selection's change handler, which GTK calls while it
/// handles the key, can tell browsing from choosing.
pub fn mark_browsing(list: &gtk::ListView, browsing: &Rc<Cell<bool>>) {
    let keys = gtk::EventControllerKey::new();
    // Before the list's own key bindings move the focus and select.
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let browsing = Rc::clone(browsing);
        move |_, key, _, _| {
            if BROWSING.contains(&key) {
                browsing.set(true);
                // Cleared once this key press is handled, whether or not it moved anything.
                let browsing = Rc::clone(&browsing);
                glib::idle_add_local_once(move || browsing.set(false));
            }
            glib::Propagation::Proceed
        }
    });
    list.add_controller(keys);
}

/// Has Enter on a row of `list` choose it, as a click does: the row is selected, and the selection's change
/// handler asks for it.
pub fn enter_chooses(list: &gtk::ListView, selection: &gtk::SingleSelection) {
    let selection = selection.clone();
    list.connect_activate(move |_, position| {
        if selection.selected() != position {
            selection.set_selected(position);
        }
    });
}
