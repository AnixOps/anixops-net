#!/usr/bin/env python3
"""Drives the native Linux window through AT-SPI, the way a screen reader sees it.

    xvfb-run -a dbus-run-session -- python3 linux-native-a11y.py <anixops-net-preview> <catalogue.json> \
        [<anixops-netd> [<screenshot directory>]]

For every language the window is started for real and checked from the outside:

- the window is named after the app;
- the sidebar offers the six pages, in order, by their names in that language;
- choosing each page through the accessibility interface shows that page;
- every control that can be operated has a name.

With the daemon's program given, the window is also run against a real daemon on a socket of its own: it
says that the core service is not running, finds the daemon when it starts, connects and disconnects with
the overview's button (the daemon is first told to send traffic out directly, since it has no subscription),
shows the engine's counters while connected, notices when the daemon stops, and finds it again. With a
directory given too, the connected window is drawn into it.

Needs at-spi2-core, gir1.2-atspi-2.0 and python3-gi.
"""

from __future__ import annotations

import json
import os
import re
import socket
import struct
import subprocess
import sys
import tempfile
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, Gio, GLib  # noqa: E402

LANGUAGES = ("zh", "zh-Hant", "en")
# (id, title wording id, the wording id of a sentence only that page shows), in sidebar order; the last one
# sits in the footer list.
PAGES = (
    ("overview", "nav.overview", "overview.node"),
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
            # The pages have lists of their own; the sidebar's two are the ones named for navigation.
            found = [node for node, _ in walk(app) if role(node) == "list" and node.get_name() == text("nav.label")]
            return found if len(found) == 2 else None

        pages, footer = until(f"the two lists of the sidebar, named {text('nav.label')!r}", lists)

        def names(sidebar):
            return [item.get_name() for item in children(sidebar) if role(item) == "list item"]

        expected = [text(title) for _, title, _ in PAGES]
        found = names(pages) + names(footer)
        if found != expected or len(names(footer)) != 1:
            dump(app)
            fail(f"[{language}] the sidebar offers {found}, expected {expected} with the last one at the bottom")

        def shown(summary: str):
            # On screen, not only in the tree: the pages that are not chosen stay built.
            return next(
                (
                    node
                    for node, _ in walk(frame)
                    if role(node) == "label"
                    and (node.get_name() or "").startswith(summary)
                    and node.get_state_set().contains(Atspi.StateType.SHOWING)
                ),
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


def stop(process: subprocess.Popen | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()


def core_ask(endpoint: str, command: str, args: dict):
    """Sends one command to the daemon as another program of this user would, and returns its answer."""

    def send(channel: socket.socket, message: dict) -> None:
        payload = json.dumps(message).encode()
        channel.sendall(struct.pack(">I", len(payload)) + payload)

    def receive(channel: socket.socket) -> dict:
        def exactly(count: int) -> bytes:
            data = b""
            while len(data) < count:
                more = channel.recv(count - len(data))
                if not more:
                    fail(f"the daemon closed the channel while {command} waited for its answer")
                data += more
            return data

        (length,) = struct.unpack(">I", exactly(4))
        return json.loads(exactly(length))

    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as channel:
        channel.settimeout(60)
        channel.connect(endpoint)
        send(channel, {"type": "hello", "protocol": 1, "role": "window", "version": "0.0.0"})
        if receive(channel).get("type") != "hello":
            fail("the daemon did not introduce itself")
        send(channel, {"type": "request", "id": 1, "command": command, "args": args})
        while True:
            # The daemon's own questions to this program (what session it runs in) go unanswered: it is
            # not a window.
            message = receive(channel)
            if message.get("type") == "response" and message.get("id") == 1:
                if "error" in message:
                    fail(f"the daemon refused {command}: {message['error']}")
                return message.get("ok")


def check_core(binary: str, daemon: str, wording: dict, shots: str | None) -> None:
    """The window against a real daemon: away, found, connected, disconnected, away again, found again."""
    language = "zh"
    text = lambda key: wording[key][language]  # noqa: E731
    before, after = text("core.connected").split("{0}")

    def connected(name: str) -> bool:
        if not (name.startswith(before) and name.endswith(after)):
            return False
        version = name[len(before):len(name) - len(after)]
        return re.fullmatch(r"\d+\.\d+\.\d+\S*", version) is not None

    with tempfile.TemporaryDirectory(prefix="anixops-net-core-") as folder:
        endpoint = os.path.join(folder, "run", "core.sock")

        def start_daemon() -> subprocess.Popen:
            return subprocess.Popen([daemon, "--dir", os.path.join(folder, "data"), "--socket", endpoint])

        window = subprocess.Popen([binary, "--language", language, "--theme", "light", "--socket", endpoint])
        core = None
        try:
            app = until("the window on the accessibility bus", lambda: application(window.pid))

            def says(wanted):
                return next(
                    (node for node, _ in walk(app) if role(node) == "label" and wanted(node.get_name() or "")),
                    None,
                )

            def away(when: str) -> None:
                until(f"the window to say {when} that the core service is not running",
                      lambda: says(lambda name: name == text("core.unavailable")))

            def found(when: str) -> str:
                return until(f"the window to say {when} which core service answers",
                             lambda: says(connected)).get_name()

            away("before the daemon started")
            core = start_daemon()
            line = found("after the daemon started")
            if core.poll() is not None:
                fail(f"the daemon stopped by itself with status {core.returncode}")

            def button(wording_id: str):
                """The overview's button under that name, once it can be pressed."""

                def probe():
                    for node, _ in walk(app):
                        if role(node) not in ("push button", "button") or node.get_name() != text(wording_id):
                            continue
                        if node.get_state_set().contains(Atspi.StateType.SENSITIVE):
                            return node
                    return None

                return until(f"the button {text(wording_id)!r} to be ready", probe)

            def press(wording_id: str) -> None:
                action = button(wording_id).get_action_iface()
                if action is None or not action.do_action(0):
                    fail(f"the button {text(wording_id)!r} could not be pressed through the accessibility interface")

            def sentence(wording_id: str) -> None:
                until(f"the window to say {text(wording_id)!r}", lambda: says(lambda name: name == text(wording_id)))

            # A daemon that was never given a subscription has no node: nothing to connect through.
            sentence("overview.state.disconnected")
            sentence("overview.node.none")
            core_ask(endpoint, "set_direct_egress", {"enabled": True})
            sentence("overview.node.direct")
            press("overview.connect")
            sentence("overview.state.connected")
            # The engine's counters: two rates, and the totals under them.
            rates = until(
                "the two rates",
                lambda: (lambda found: found if len(found) == 2 else None)(
                    [
                        node.get_name()
                        for node, _ in walk(app)
                        if role(node) == "label" and re.fullmatch(r"\d+(\.\d)? [KMGT]?B/s", node.get_name() or "")
                    ]
                ),
            )
            if shots:
                # A second window, as short-lived as every screenshot: it finds the daemon by itself.
                picture = os.path.join(shots, "core-connected-zh-light.png")
                subprocess.run(
                    [binary, "--language", language, "--theme", "light", "--socket", endpoint, "--screenshot", picture],
                    check=True,
                    timeout=60,
                )
                if not os.path.getsize(picture):
                    fail(f"{picture} is empty")
            press("overview.disconnect")
            sentence("overview.state.disconnected")
            button("overview.connect")
            stop(core)
            core = None
            away("after the daemon stopped")
            core = start_daemon()
            found("after the daemon started again")
            print(f"[core] not running, found ({line!r}), connected ({', '.join(rates)}), disconnected, "
                  "stopped, found again")
        finally:
            stop(window)
            stop(core)


def main() -> None:
    if not 3 <= len(sys.argv) <= 5:
        fail("usage: linux-native-a11y.py <anixops-net-preview> <catalogue.json> [<anixops-netd> [<screenshots>]]")
    binary, catalogue = sys.argv[1:3]
    daemon = sys.argv[3] if len(sys.argv) > 3 else None
    shots = sys.argv[4] if len(sys.argv) > 4 else None
    with open(catalogue, encoding="utf-8") as file:
        wording = json.load(file)
    Atspi.init()
    enable_accessibility()
    for language in LANGUAGES:
        check(binary, wording, language)
    if daemon:
        check_core(binary, daemon, wording, shots)


if __name__ == "__main__":
    main()
