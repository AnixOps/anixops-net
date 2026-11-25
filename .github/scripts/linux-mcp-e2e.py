#!/usr/bin/env python3
"""anixops-mcp against the real Linux daemon (docs/mcp-desktop.md §9, item 5).

Usage: linux-mcp-e2e.py <anixops-netd> <anixops-mcp>

Starts the daemon on a socket and a data folder of its own and speaks MCP with anixops-mcp as an AI tool would:
access is off until a window turns it on; then the state and the nodes are read, without the node's address or
credentials; an engine request waits for a connection; an action is refused while only reading is allowed; a window's command is never reached; and once the daemon
stops, every tool says it is not running.
"""

import json
import os
import socket
import struct
import subprocess
import sys
import tempfile
import time

NODE = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.9:11111#Test"


def fail(message: str) -> None:
    print(f"::error::{message}")
    sys.exit(1)


def window_ask(endpoint: str, command: str, args: dict):
    """One command to the daemon as the user's window sends it."""

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
            message = receive(channel)
            if message.get("type") == "response" and message.get("id") == 1:
                if "error" in message:
                    fail(f"the daemon refused {command}: {message['error']}")
                return message.get("ok")


def mcp(program: str, endpoint: str, calls: list) -> list:
    """Starts anixops-mcp, says hello, makes each tool call (name, arguments) and returns the results in order."""
    lines = [
        {"jsonrpc": "2.0", "id": 0, "method": "initialize",
         "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "ci", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
    ]
    for index, (name, arguments) in enumerate(calls, start=1):
        lines.append({"jsonrpc": "2.0", "id": index, "method": "tools/call",
                      "params": {"name": name, "arguments": arguments}})
    text = "".join(json.dumps(line) + "\n" for line in lines)
    done = subprocess.run([program, "--socket", endpoint], input=text, capture_output=True, text=True, timeout=120)
    if done.returncode != 0:
        fail(f"anixops-mcp ended with {done.returncode}: {done.stderr}")
    answers = {}
    for line in done.stdout.splitlines():
        message = json.loads(line)
        answers[message["id"]] = message
    if answers.get(0, {}).get("result", {}).get("protocolVersion") != "2025-06-18":
        fail(f"anixops-mcp did not answer initialize: {answers.get(0)}")
    results = []
    for index, (name, _) in enumerate(calls, start=1):
        result = answers.get(index, {}).get("result")
        if result is None:
            fail(f"anixops-mcp gave no result for {name}: {answers.get(index)}")
        results.append(result)
    return results


def text(result: dict) -> str:
    return result["content"][0]["text"]


def expect_error(result: dict, words: str, what: str) -> None:
    if result.get("isError") is not True or words not in text(result):
        fail(f"{what}: expected an error with {words!r}, got {result}")


def expect_ok(result: dict, what: str) -> dict:
    if result.get("isError") is not False:
        fail(f"{what}: expected a result, got {result}")
    return json.loads(text(result))


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: linux-mcp-e2e.py <anixops-netd> <anixops-mcp>")
    daemon, program = sys.argv[1], sys.argv[2]
    folder = tempfile.mkdtemp(prefix="anixops-mcp-e2e-")
    endpoint = os.path.join(folder, "core.sock")
    process = subprocess.Popen([daemon, "--dir", os.path.join(folder, "data"), "--socket", endpoint])
    try:
        deadline = time.monotonic() + 60
        while not os.path.exists(endpoint):
            if process.poll() is not None or time.monotonic() > deadline:
                fail("the daemon did not start listening")
            time.sleep(0.2)

        # Off until the user turns it on in a window.
        (status,) = mcp(program, endpoint, [("get_status", {})])
        expect_error(status, "has not allowed access by AI tools", "get_status with access off")

        window_ask(endpoint, "import_subscription_text", {"name": "Home", "content": NODE})
        window_ask(endpoint, "set_agent_access", {"read": True, "content": "summary"})
        status, nodes, rules, logs, connect = mcp(program, endpoint, [
            ("get_status", {}),
            ("list_nodes", {}),
            ("check_rules", {"rules": ["FINAL,DIRECT"]}),
            ("get_logs", {"limit": 20}),
            ("connect", {}),
        ])
        status = expect_ok(status, "get_status")
        if status.get("connection") != "disconnected" or status.get("node") != "Test":
            fail(f"get_status: {status}")
        listed = json.dumps(expect_ok(nodes, "list_nodes"))
        if '"Test"' not in listed:
            fail(f"list_nodes does not list the node: {listed}")
        for secret in ("203.0.113.9", "11111", "YWVz"):
            if secret in listed or secret in json.dumps(status):
                fail(f"the node's {secret} reached the AI tool")
        expect_error(rules, "not connected", "check_rules while not connected")
        expect_ok(logs, "get_logs")
        # Reading does not allow actions; the daemon offers no switch for them yet.
        expect_error(connect, "has not allowed actions by AI tools", "connect with only reading allowed")

        # Turned off again: refused at once, in the daemon.
        window_ask(endpoint, "set_agent_access", {"read": False, "content": "summary"})
        (status,) = mcp(program, endpoint, [("get_status", {})])
        expect_error(status, "has not allowed access by AI tools", "get_status after access was turned off")
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()

    (status,) = mcp(program, endpoint, [("get_status", {})])
    expect_error(status, "not running", "get_status without the daemon")
    print("anixops-mcp: the daemon answered as expected")


if __name__ == "__main__":
    main()
