#!/usr/bin/env python3
"""Drives the native Linux window through AT-SPI, the way a screen reader sees it.

    xvfb-run -a dbus-run-session -- python3 linux-native-a11y.py <anixops-net-preview> <catalogue.json>

For every language the window is started for real and checked from the outside:

- the window is named after the app;
- the sidebar offers the six pages, in order, by their names in that language;
- choosing each page through the accessibility interface shows that page;
- every control that can be operated has a name.

Needs at-spi2-core, gir1.2-atspi-2.0 and python3-gi.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, Gio, GLib  # noqa: E402

LANGUAGES = ("zh", "zh-Hant", "en")
# (id, title wording id, summary wording id), in sidebar order; the last one sits in the footer list.
PAGES = (
    ("overview", "nav.overview", "page.overview.summary"),
    ("proxies", "nav.proxies", "page.proxies.summary"),
    ("rules", "nav.rules", "page.rules.summary"),
    ("extensions", "nav.extensions", "page.extensions.summary"),
    ("activity", "nav.activity", "page.activity.summary"),
    ("settings", "nav.settings", "page.settings.summary"),
)
WAIT = 20.0


def fail(message: str) -> None:
    sys.exit(f"linux-native-a11y: {message}")


def role(node: Atspi.Accessible) -> str:
    return node.get_role_name() or ""


def children(node: Atspi.Accessible) -> list[Atspi.Accessible]:
    found = []
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            found.append(child)
    return found


def walk(node: Atspi.Accessible, depth: int = 0):
    yield node, depth
    if depth > 60:
        return
    for child in children(node):
        yield from walk(child, depth + 1)


def until(what: str, probe):
    """Calls probe until it returns something, for at most WAIT seconds."""
    deadline = time.monotonic() + WAIT
    last = None
    while time.monotonic() < deadline:
        try:
            found = probe()
            if found:
                return found
        except GLib.Error as error:  # the tree changes under us while the window builds itself
            last = error
        time.sleep(0.1)
    fail(f"timed out waiting for {what}" + (f" ({last.message})" if last else ""))


def application(pid: int) -> Atspi.Accessible | None:
    desktop = Atspi.get_desktop(0)
    for app in children(desktop):
        if app.get_process_id() == pid:
            return app
    return None


def dump(app: Atspi.Accessible) -> None:
    for node, depth in walk(app):
        print(f"{'  ' * depth}{role(node)}: {node.get_name()!r}")


def enable_accessibility() -> None:
    """Says on the accessibility bus that an assistive technology is listening. Toolkits that only build
    their accessible tree on demand look at this; it does no harm to the ones that always build it."""
    try:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        bus.call_sync(
            "org.a11y.Bus",
            "/org/a11y/bus",
            "org.freedesktop.DBus.Properties",
            "Set",
            GLib.Variant("(ssv)", ("org.a11y.Status", "IsEnabled", GLib.Variant("b", True))),
            None,
            Gio.DBusCallFlags.NONE,
            5000,
            None,
        )
    except GLib.Error as error:
        print(f"note: could not set org.a11y.Status.IsEnabled: {error.message}")


def check(binary: str, wording: dict, language: str) -> None:
    text = lambda key: wording[key][language]  # noqa: E731
    process = subprocess.Popen([binary, "--language", language, "--theme", "light"])
    try:
        app = until("the window on the accessibility bus", lambda: application(process.pid))

        frame = until(
            "the window",
            lambda: next((node for node, _ in walk(app) if role(node) in ("frame", "window")), None),
        )
        if frame.get_name() != text("app.name"):
            dump(app)
            fail(f"[{language}] the window is named {frame.get_name()!r}, not {text('app.name')!r}")

        def lists():
            found = [node for node, _ in walk(app) if role(node) == "list"]
            return found if len(found) == 2 else None

        pages, footer = until("the two lists of the sidebar", lists)
        for sidebar in (pages, footer):
            if sidebar.get_name() != text("nav.label"):
                dump(app)
                fail(f"[{language}] a sidebar list is named {sidebar.get_name()!r}, not {text('nav.label')!r}")

        def names(sidebar):
            return [item.get_name() for item in children(sidebar) if role(item) == "list item"]

        expected = [text(title) for _, title, _ in PAGES]
        found = names(pages) + names(footer)
        if found != expected or len(names(footer)) != 1:
            dump(app)
            fail(f"[{language}] the sidebar offers {found}, expected {expected} with the last one at the bottom")

        def shown(summary: str):
            return next(
                (node for node, _ in walk(frame) if role(node) == "label" and (node.get_name() or "").startswith(summary)),
                None,
            )

        # The window opens on the first page.
        until(f"[{language}] the first page", lambda: shown(text(PAGES[0][2])))

        # Backwards, so that the first step already leaves the page the window opened on.
        for index in reversed(range(len(PAGES))):
            _, title, summary = PAGES[index]
            sidebar, at = (footer, 0) if index == len(PAGES) - 1 else (pages, index)
            selection = sidebar.get_selection_iface()
            if selection is None:
                fail(f"[{language}] the sidebar cannot be operated through the accessibility interface")
            if not selection.select_child(at):
                fail(f"[{language}] choosing {text(title)!r} was refused")
            until(f"[{language}] the page {text(title)!r}", lambda: shown(text(summary)))
            # Exactly one row of the two lists is the chosen one.
            chosen = [
                item.get_name()
                for part in (pages, footer)
                for item in children(part)
                if item.get_state_set().contains(Atspi.StateType.SELECTED)
            ]
            if chosen != [text(title)]:
                fail(f"[{language}] after choosing {text(title)!r} the chosen rows are {chosen}")

        # Every control that can be operated says what it is.
        operable = ("push button", "button", "toggle button", "check box", "radio button", "list item", "entry",
                    "combo box", "menu item", "link", "slider", "spin button", "switch", "page tab")
        unnamed = []
        controls = 0
        for node, _ in walk(frame):
            if role(node) not in operable or not node.get_state_set().contains(Atspi.StateType.SHOWING):
                continue
            controls += 1
            if not (node.get_name() or "").strip() and not (node.get_description() or "").strip():
                unnamed.append(role(node))
        if unnamed:
            dump(app)
            fail(f"[{language}] controls without a name: {unnamed}")
        print(f"[{language}] window, sidebar and {len(PAGES)} pages reached; {controls} controls, all named")
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: linux-native-a11y.py <anixops-net-preview> <catalogue.json>")
    binary, catalogue = sys.argv[1:]
    with open(catalogue, encoding="utf-8") as file:
        wording = json.load(file)
    Atspi.init()
    enable_accessibility()
    for language in LANGUAGES:
        check(binary, wording, language)


if __name__ == "__main__":
    main()
