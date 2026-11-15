const assert = require("node:assert/strict");
const test = require("node:test");
const { acknowledged, matches, review } = require("./ack.cjs");

test("reads the names on the acknowledgement lines", () => {
  const body = [
    "## 改了什么",
    "检查器改为标签页。",
    "",
    "界面变化：debug-requests-light, debug-requests-dark、`debug-inspector-*`",
    "- UI changes: home-light.png home-dark",
  ].join("\n");
  assert.deepEqual(acknowledged(body), ["debug-requests-light", "debug-requests-dark", "debug-inspector-*", "home-light", "home-dark"]);
});

test("ignores the phrase anywhere but the start of a line, and odd tokens", () => {
  assert.deepEqual(acknowledged("这个 PR 没有界面变化：home-light"), []);
  assert.deepEqual(acknowledged("界面变化： 无"), []);
  // Only plain shot names and patterns are taken; anything else on the line is dropped.
  assert.deepEqual(acknowledged("界面变化：home-light; rm -rf / ../x $(id)"), ["home-light", "rm"]);
  assert.deepEqual(acknowledged(null), []);
  assert.deepEqual(acknowledged(""), []);
});

test("does not repeat a name and handles Windows line endings", () => {
  assert.deepEqual(acknowledged("界面变化：a-light, a-light\r\n界面变化：b-dark\r\n"), ["a-light", "b-dark"]);
});

test("a star matches any run of characters and nothing else is special", () => {
  assert.equal(matches("debug-*", "debug-requests-light"), true);
  assert.equal(matches("*-dark", "home-dark"), true);
  assert.equal(matches("*", "anything"), true);
  assert.equal(matches("home-light", "home-light"), true);
  assert.equal(matches("home-light", "home-light-2"), false);
  assert.equal(matches("home.light", "homeXlight"), false);
  assert.equal(matches("debug-*-light", "debug-requests-dark"), false);
});

test("splits changes into covered and uncovered and reports unused acknowledgements", () => {
  const result = review(["home-light", "home-dark", "proxies-light"], "界面变化：home-*, rules-light");
  assert.deepEqual(result, { covered: ["home-light", "home-dark"], uncovered: ["proxies-light"], unused: ["rules-light"] });
  assert.deepEqual(review([], "界面变化：home-light"), { covered: [], uncovered: [], unused: ["home-light"] });
  assert.deepEqual(review(["home-light"], ""), { covered: [], uncovered: ["home-light"], unused: [] });
});
