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
shows the engine's counters while connected, imports share links on the proxies page, searches the node
list, chooses another node, tests the nodes' delay, opens the list of subscriptions and updates, renames and
removes one that a server of this script's own hands out, notices when the daemon stops, and finds it again.
With a directory given too, the connected window, the proxies page and the list of subscriptions are drawn
into it.

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
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, Gio, GLib  # noqa: E402

LANGUAGES = ("zh", "zh-Hant", "en")
# (id, title wording id, the wording id of a sentence only that page shows), in sidebar order; the last one
# sits in the footer list.
PAGES = (
    ("overview", "nav.overview", "overview.node"),
    ("proxies", "nav.proxies", "proxies.import.link"),
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


# Two nodes nobody answers at (addresses reserved for documentation): importing, listing and choosing them
# needs no server, and a delay test of them ends without an answer.
FIRST_NODE = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.9:11111#Test"
SECOND_NODE = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.10:11111#Second"


def check_proxies(app, binary: str, endpoint: str, text, button, press, sentence, shots: str | None) -> str:
    """The proxies page against the real daemon: import, list, search, choose a node, test the delays."""

    def showing(node) -> bool:
        return node.get_state_set().contains(Atspi.StateType.SHOWING)

    def label(wanted: str):
        return next(
            (node for node, _ in walk(app) if role(node) == "label" and node.get_name() == wanted and showing(node)),
            None,
        )

    def editable(name: str):
        """The text box under that name, as something text can be put into."""

        def probe():
            for node, _ in walk(app):
                if node.get_name() != name or not showing(node):
                    continue
                for candidate, _ in walk(node):
                    try:
                        iface = candidate.get_editable_text_iface()
                    except GLib.Error:
                        iface = None
                    if iface is not None:
                        return iface
            return None

        return until(f"the text box {name!r}", probe)

    def write(name: str, value: str) -> None:
        if not editable(name).set_text_contents(value):
            fail(f"the text box {name!r} did not take {value!r} through the accessibility interface")

    def node_list():
        return next(
            (node for node, _ in walk(app) if node.get_name() == text("proxies.list") and showing(node)),
            None,
        )

    def items() -> list:
        found = node_list()
        if found is None:
            return []
        return [node for node, _ in walk(found) if role(node) == "list item" and showing(node)]

    def item(name: str):
        return next((node for node in items() if node.get_name() == name), None)

    def count(total: int, shown: int | None = None) -> str:
        if shown is None or shown == total:
            return text("proxies.count.one") if total == 1 else text("proxies.count").replace("{0}", str(total))
        return text("proxies.count.shown").replace("{0}", str(shown)).replace("{1}", str(total))

    # The second row of the sidebar's first list.
    sidebar = until(
        "the sidebar",
        lambda: next((node for node, _ in walk(app) if role(node) == "list" and node.get_name() == text("nav.label")),
                     None),
    )
    if not sidebar.get_selection_iface().select_child(1):
        fail("choosing the proxies page was refused")
    # A daemon that was never given a subscription has no node.
    until("the proxies page to say that there are no nodes", lambda: label(text("proxies.empty")))

    # Import one share link the way the user does: write it into the box and press the button.
    write(text("proxies.import.link"), FIRST_NODE)
    press("proxies.import")
    until("the imported node in the list", lambda: item("Test"))
    until(f"the page to say {count(1)!r}", lambda: label(count(1)))
    button("proxies.import")

    # A second node, given to the daemon by another program: the window shows it without being told.
    core_ask(endpoint, "import_subscription", {"location": FIRST_NODE + "\n" + SECOND_NODE})
    until("the second node in the list", lambda: item("Second"))
    total = core_ask(endpoint, "proxies", {})["total"]
    until(f"the page to say {count(total)!r}", lambda: label(count(total)))

    # Searching leaves the nodes the daemon says it leaves.
    left = core_ask(endpoint, "proxies", {"query": "second"})["shown"]
    if not 0 < left < total:
        fail(f"searching 'second' leaves {left} of {total} nodes at the daemon")
    write(text("proxies.search"), "second")
    until(f"the page to say {count(total, left)!r}", lambda: label(count(total, left)))
    until("the search to leave only the second node", lambda: item("Test") is None and item("Second") is not None)
    write(text("proxies.search"), "")
    until(f"the page to say {count(total)!r} again", lambda: label(count(total)))
    until("the first node to be back", lambda: item("Test"))

    # Choosing a node that is not in use makes the daemon use it.
    def in_use() -> list[str]:
        answer = core_ask(endpoint, "proxies", {})
        return [node["label"] for section in answer["sections"] for node in section["nodes"] if node["selected"]]

    before = in_use()
    names = [node.get_name() for node in items()]
    target = next((name for name in names if name not in before), None)
    if target is None:
        fail(f"every listed node ({names}) is already in use ({before})")
    selection = node_list().get_selection_iface()
    if selection is None:
        dump(app)
        fail("the node list cannot be operated through the accessibility interface")
    if not selection.select_child(names.index(target)):
        fail(f"choosing the node {target!r} was refused")
    until(f"the daemon to use the node {target!r} (it used {before})", lambda: in_use() == [target])
    until(
        f"the row of {target!r} to be the selected one",
        lambda: [node.get_name() for node in items() if node.get_state_set().contains(Atspi.StateType.SELECTED)]
        == [target],
    )

    # Nobody answers at these nodes: the test ends and every node says so.
    press("proxies.test")

    def unanswered():
        found = [
            node
            for node, _ in walk(app)
            if role(node) == "label" and node.get_name() == text("proxies.delay.failed") and showing(node)
        ]
        return found if len(found) == len(items()) else None

    until("every node to say that its delay test got no answer", unanswered)
    button("proxies.test")
    if shots:
        picture = os.path.join(shots, "core-proxies-zh-light.png")
        subprocess.run(
            [binary, "--language", "zh", "--theme", "light", "--socket", endpoint, "--page", "proxies",
             "--screenshot", picture],
            check=True,
            timeout=60,
        )
        if not os.path.getsize(picture):
            fail(f"{picture} is empty")
    return f"imported, {total} nodes, searched ({left}), chose {target!r}, tested"


# What the script's own subscription server hands out: one node at first, two from the second download on.
THIRD_NODE = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.20:11111#Alpha"
FOURTH_NODE = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.21:11111#Beta"
# 1000 bytes of 5000 used, and the last second of 2033-05-17 UTC as the end: the day it expires is the 18th.
USERINFO = "upload=100; download=900; total=5000; expire=1999999999"


def subscription_server():
    """A subscription on this machine. Returns the server, its link and the list its downloads are counted in."""
    hits: list[str] = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            hits.append(self.path)
            body = (THIRD_NODE if len(hits) == 1 else THIRD_NODE + "\n" + FOURTH_NODE).encode()
            self.send_response(200)
            self.send_header("content-type", "text/plain; charset=utf-8")
            self.send_header("content-length", str(len(body)))
            self.send_header("subscription-userinfo", USERINFO)
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_address[1]}/nodes", hits


def check_subscriptions(app, binary: str, endpoint: str, text, press, shots: str | None) -> str:
    """The list of subscriptions against the real daemon: what a row says, update, rename, remove."""

    def showing(node) -> bool:
        return node.get_state_set().contains(Atspi.StateType.SHOWING)

    def listed() -> list[dict]:
        return core_ask(endpoint, "subscriptions", {})["subscriptions"]

    def one(identity: str) -> dict | None:
        return next((row for row in listed() if row["id"] == identity), None)

    def rows() -> list:
        found = next(
            (
                node
                for node, _ in walk(app)
                if role(node) == "list" and node.get_name() == text("proxies.subscriptions") and showing(node)
            ),
            None,
        )
        if found is None:
            return []
        return [node for node, _ in walk(found) if role(node) == "list item" and showing(node)]

    def says(row) -> list[str]:
        return [node.get_name() or "" for node, _ in walk(row) if role(node) == "label" and showing(node)]

    def row(title: str):
        return next((found for found in rows() if title in says(found)), None)

    def row_button(title: str, wording_id: str):
        """The button of that name in the row of that title, once it can be pressed."""

        def probe():
            found = row(title)
            if found is None:
                return None
            for node, _ in walk(found):
                if role(node) in ("push button", "button") and node.get_name() == text(wording_id) \
                        and showing(node) and node.get_state_set().contains(Atspi.StateType.SENSITIVE):
                    return node
            return None

        return until(f"the button {text(wording_id)!r} of the subscription {title!r}", probe)

    def push(node, what: str) -> None:
        action = node.get_action_iface()
        if action is None or not action.do_action(0):
            fail(f"{what} could not be pressed through the accessibility interface")

    def question(heading: str, answer: str):
        """The button of that name in the question under that heading, not one of the same name elsewhere."""

        def probe():
            asked = next(
                (node for node, _ in walk(app) if role(node) == "label" and node.get_name() == heading
                 and showing(node)),
                None,
            )
            # Upwards from the heading to the first thing that also holds the question's "cancel".
            while asked is not None:
                buttons = [
                    node for node, _ in walk(asked)
                    if role(node) in ("push button", "button") and showing(node)
                ]
                if any(node.get_name() == text("action.cancel") for node in buttons):
                    return next((node for node in buttons if node.get_name() == answer), None)
                asked = asked.get_parent()
            return None

        return until(f"the answer {answer!r} to {heading!r}", probe)

    server, link, hits = subscription_server()
    try:
        before = {row["id"] for row in listed()}
        nodes_before = core_ask(endpoint, "proxies", {})["total"]
        core_ask(endpoint, "import_subscription", {"location": link})
        added = [row for row in listed() if row["id"] not in before]
        if len(added) != 1:
            fail(f"importing {link} added {len(added)} subscriptions at the daemon")
        mine = added[0]
        identity, title = mine["id"], mine["name"]
        expected = {"pasted": False, "refreshable": True, "nodes": 1, "used": 1000, "total": 5000,
                    "expireDate": "2033-05-18", "expired": False, "error": None}
        wrong = {name: mine.get(name) for name, value in expected.items() if mine.get(name) != value}
        if wrong:
            fail(f"the daemon describes the subscription from {link} with {wrong}, expected {expected}")

        press("proxies.subscriptions")
        until(f"the row of the subscription {title!r}", lambda: row(title))

        def details(*parts: str):
            return lambda: (lambda found: found is not None
                            and any(all(part in line for part in parts) for line in says(found)))(row(title))

        nodes = lambda count: (  # noqa: E731
            text("proxies.count.one") if count == 1 else text("proxies.count").replace("{0}", str(count))
        )
        used = text("subscriptions.usage").replace("{0}", "1000 B").replace("{1}", "4.9 KB")
        expires = text("subscriptions.expire").replace("{0}", "2033-05-18")
        until(f"the row to say {nodes(1)!r}, {used!r} and {expires!r} in one line", details(nodes(1), used, expires))

        # Pasted links have a row too, named in the window's language, and nothing to download again.
        pasted = until("the row of the pasted links", lambda: row(text("proxies.group.pasted")))
        if any(role(node) in ("push button", "button") and node.get_name() == text("subscriptions.update")
               and showing(node) for node, _ in walk(pasted)):
            fail("the pasted links offer to be updated")

        # Update: the server is asked again and now hands out two nodes.
        push(row_button(title, "subscriptions.update"), "the update button")
        until("the subscription to be downloaded again", lambda: len(hits) >= 2)
        until("the daemon to hold the subscription's two nodes", lambda: (one(identity) or {}).get("nodes") == 2)
        until(f"the row to say {nodes(2)!r}", details(nodes(2)))
        row_button(title, "subscriptions.update")

        # Rename: the question starts with the name as it is and takes another.
        renamed = "My nodes"
        push(row_button(title, "subscriptions.rename"), "the rename button")

        def name_box():
            for node, _ in walk(app):
                if node.get_name() != text("subscriptions.name") or not showing(node):
                    continue
                for candidate, _ in walk(node):
                    try:
                        iface = candidate.get_editable_text_iface()
                    except GLib.Error:
                        iface = None
                    if iface is not None:
                        return candidate, iface
            return None

        box, editable = until(f"the text box {text('subscriptions.name')!r}", name_box)
        text_iface = box.get_text_iface()
        if text_iface is not None:
            written = Atspi.Text.get_text(text_iface, 0, Atspi.Text.get_character_count(text_iface))
            if written != title:
                fail(f"the box for the new name starts with {written!r}, not {title!r}")
        if not editable.set_text_contents(renamed):
            fail("the box for the new name did not take a name through the accessibility interface")
        push(question(text("subscriptions.rename"), text("subscriptions.save")), "the save button")
        until(f"the daemon to call the subscription {renamed!r}", lambda: (one(identity) or {}).get("name") == renamed)
        until(f"the row of {renamed!r}", lambda: row(renamed) is not None and row(title) is None)

        if shots:
            picture = os.path.join(shots, "core-subscriptions-zh-light.png")
            subprocess.run(
                [binary, "--language", "zh", "--theme", "light", "--socket", endpoint, "--page", "proxies",
                 "--show", "subscriptions", "--screenshot", picture],
                check=True,
                timeout=60,
            )
            if not os.path.getsize(picture):
                fail(f"{picture} is empty")

        # Remove: asked first, and "cancel" leaves everything as it is.
        heading = text("subscriptions.remove.confirm").replace("{0}", renamed)
        push(row_button(renamed, "subscriptions.remove"), "the remove button")
        push(question(heading, text("action.cancel")), "the cancel button")
        until("the question to be gone", lambda: not any(
            role(node) == "label" and node.get_name() == heading and showing(node) for node, _ in walk(app)))
        if one(identity) is None or row(renamed) is None:
            fail("cancelling the removal removed the subscription")
        push(row_button(renamed, "subscriptions.remove"), "the remove button")
        push(question(heading, text("subscriptions.remove")), "the remove answer")
        until("the daemon to have removed the subscription", lambda: one(identity) is None)
        until("the row to be gone", lambda: rows() and row(renamed) is None)
        until("the subscription's nodes to be gone",
              lambda: core_ask(endpoint, "proxies", {})["total"] == nodes_before)
        return f"subscription {title!r}: listed, updated ({len(hits)} downloads), renamed, removed"
    finally:
        server.shutdown()


# A subscription with rules and policy groups, as the script's own server hands it out.
ROUTED = """\
proxies:
  - name: HK
    type: ss
    server: 203.0.113.30
    port: 11111
    cipher: aes-256-gcm
    password: f43c0eee-13b9-4f07-bec9-d4b744141503
  - name: JP
    type: ss
    server: 203.0.113.31
    port: 11112
    cipher: aes-256-gcm
    password: f43c0eee-13b9-4f07-bec9-d4b744141504
proxy-groups:
  - name: Proxy
    type: select
    proxies:
      - Auto
      - HK
      - DIRECT
  - name: Auto
    type: url-test
    proxies:
      - JP
      - HK
    url: http://www.gstatic.com/generate_204
    interval: 300
rules:
  - DOMAIN-SUFFIX,example.org,DIRECT
  - MATCH,Proxy
"""


def routed_server():
    """A subscription with policy groups on this machine. Returns the server and its link."""

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = ROUTED.encode()
            self.send_response(200)
            self.send_header("content-type", "text/yaml; charset=utf-8")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_address[1]}/routed"


def check_groups(app, binary: str, endpoint: str, text, press, shots: str | None) -> str:
    """The policy groups against the real daemon: what a row says, and choosing a member of a group."""

    def showing(node) -> bool:
        return node.get_state_set().contains(Atspi.StateType.SHOWING)

    def group(tag: str) -> dict | None:
        return next((row for row in core_ask(endpoint, "groups", {})["groups"] if row["tag"] == tag), None)

    def listing(name: str):
        """The list under that name that is on the screen now: the first thing of that name with rows in it."""
        for node, _ in walk(app):
            if node.get_name() != name or not showing(node) or role(node) in ("push button", "button", "label"):
                continue
            if any(role(inner) == "list item" for inner, _ in walk(node)):
                return node
        return None

    def rows(name: str) -> list:
        found = listing(name)
        if found is None:
            return []
        return [node for node, _ in walk(found) if role(node) == "list item" and showing(node)]

    def says(row) -> list[str]:
        return [node.get_name() or "" for node, _ in walk(row) if role(node) == "label" and showing(node)]

    def row(tag: str):
        return next((found for found in rows(text("proxies.groups")) if tag in says(found)), None)

    def chooser(tag: str):
        """The button that opens the members of that group. It is called by the group's name: the row names
        the widget that pressing it activates after its title."""
        found = row(tag)
        if found is None:
            return None
        return next(
            (node for node, _ in walk(found)
             if role(node) in ("push button", "button") and node.get_name() == tag and showing(node)),
            None,
        )

    def buttons(tag: str) -> list:
        found = row(tag)
        return [] if found is None else [
            node for node, _ in walk(found) if role(node) in ("push button", "button") and showing(node)
        ]

    def opener():
        """The page's button that opens the groups, if it is on the screen."""
        return next(
            (node for node, _ in walk(app)
             if role(node) in ("push button", "button") and node.get_name() == text("proxies.groups")
             and showing(node)),
            None,
        )

    if opener() is not None:
        fail("the page offers policy groups before a subscription with groups is in use")
    server, link = routed_server()
    try:
        before = {row["id"] for row in core_ask(endpoint, "subscriptions", {})["subscriptions"]}
        core_ask(endpoint, "import_subscription", {"location": link})
        added = [row["id"] for row in core_ask(endpoint, "subscriptions", {})["subscriptions"]
                 if row["id"] not in before]
        if len(added) != 1:
            fail(f"importing {link} added {len(added)} subscriptions at the daemon")
        proxy = group("Proxy")
        if proxy is None or not proxy["choosable"] or proxy["selectedLabel"] != "Auto":
            fail(f"the daemon describes the group Proxy of {link} as {proxy}")
        automatic = group("Auto")
        if automatic is None or automatic["choosable"] or automatic["kind"] != "auto":
            fail(f"the daemon describes the group Auto of {link} as {automatic}")

        until("the page to offer the policy groups", opener)
        press("proxies.groups")
        using = text("groups.kind.select") + " · " + text("groups.using").replace("{0}", "Auto")
        until(f"the row of Proxy to say {using!r}", lambda: (lambda found: found is not None
                                                             and using in says(found))(row("Proxy")))
        until("the row of Auto to say that the group chooses the fastest",
              lambda: (lambda found: found is not None
                       and any(line.startswith(text("groups.kind.auto")) for line in says(found)))(row("Auto")))
        # Only a group the user chooses in opens its members.
        if buttons("Auto"):
            fail("the group Auto, which chooses by itself, offers to choose a member")

        button = until("the button that opens the members of Proxy", lambda: chooser("Proxy"))
        action = button.get_action_iface()
        if action is None or not action.do_action(0):
            fail("the members of Proxy could not be opened through the accessibility interface")
        wanted = [member["label"] for member in proxy["members"]]
        try:
            until(f"the members {wanted} of Proxy", lambda: [node.get_name() for node in rows("Proxy")] == wanted)
        except SystemExit:
            # What the window shows instead, for whoever reads the log.
            dump(app)
            raise

        def chosen() -> list[str]:
            return [node.get_name() for node in rows("Proxy")
                    if node.get_state_set().contains(Atspi.StateType.SELECTED)]

        until("the member Auto to be the selected one", lambda: chosen() == ["Auto"])
        selection = listing("Proxy").get_selection_iface()
        if selection is None:
            dump(app)
            fail("the members of a group cannot be chosen through the accessibility interface")
        if not selection.select_child(wanted.index("HK")):
            fail("choosing the member HK was refused")
        until("the daemon to use HK in the group Proxy",
              lambda: (group("Proxy") or {}).get("selectedLabel") == "HK")
        until("the member HK to be the selected one", lambda: chosen() == ["HK"])

        if shots:
            picture = os.path.join(shots, "core-groups-zh-light.png")
            subprocess.run(
                [binary, "--language", "zh", "--theme", "light", "--socket", endpoint, "--page", "proxies",
                 "--show", "groups", "--screenshot", picture],
                check=True,
                timeout=60,
            )
            if not os.path.getsize(picture):
                fail(f"{picture} is empty")

        # Without the subscription there are no groups: the dialog goes, and the page's button with it.
        core_ask(endpoint, "remove_subscription", {"id": added[0]})
        until("the daemon to have no policy groups", lambda: not core_ask(endpoint, "groups", {})["groups"])
        until("the members and the groups to be gone",
              lambda: listing("Proxy") is None and listing(text("proxies.groups")) is None)
        until("the page to offer no policy groups", lambda: opener() is None)
        return f"groups {[row['tag'] for row in (proxy, automatic)]}: listed, chose 'HK' in 'Proxy', gone"
    finally:
        server.shutdown()


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
            nodes = check_proxies(app, binary, endpoint, text, button, press, sentence, shots)
            groups = check_groups(app, binary, endpoint, text, press, shots)
            subscriptions = check_subscriptions(app, binary, endpoint, text, press, shots)
            stop(core)
            core = None
            away("after the daemon stopped")
            core = start_daemon()
            found("after the daemon started again")
            print(f"[core] not running, found ({line!r}), connected ({', '.join(rates)}), disconnected, "
                  f"proxies ({nodes}), {groups}, {subscriptions}, stopped, found again")
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
