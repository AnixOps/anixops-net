// Verifies a Core snapshot projected by the Rust client-view example at the desktop bridge boundary.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");

const [url, projectionPath, expectationsPath] = process.argv.slice(2);
const projection = JSON.parse(fs.readFileSync(projectionPath, "utf8"));
const expectations = JSON.parse(fs.readFileSync(expectationsPath, "utf8"));
const mock = fs.readFileSync(path.join(__dirname, "mock.js"), "utf8");

const warningTexts = {
  sameNetwork: "此网络有同名记录。",
  acrossRegions: "此网络在多个地区有同名记录。",
  localRegion: "本地区有同名成员。",
};

async function verifyCopy(page, container, label, value) {
  const copy = container.getByRole("button", { name: label, exact: true });
  if (!value) {
    assert.equal(await copy.count(), 0, `${label} should be absent`);
    return;
  }
  await container.getByText(value, { exact: true }).waitFor();
  await page.evaluate(() => navigator.clipboard.writeText(""));
  await copy.click();
  await page.waitForFunction((expected) => navigator.clipboard.readText().then((actual) => actual === expected), value);
  await copy.locator('[role="status"]').filter({ hasText: "已拷贝" }).waitFor({ state: "attached" });
}

async function run() {
  assert.ok(expectations.members?.length, "at least one expected member");
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    page.setDefaultTimeout(10000);
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await page.addInitScript({ content: `${mock}
      const projection = ${JSON.stringify(projection)};
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command === "runtime_control" && args.request?.cmd === "mesh_status") return Promise.resolve(projection.status);
        if (Object.hasOwn(projection, command)) return Promise.resolve(projection[command]);
        if (command.startsWith("view.mesh_")) throw new Error("Missing Rust projection for " + command);
        return original(command, args);
      };
      localStorage.setItem("anixops.app-mode", "proxy");
      localStorage.setItem("anixops.language", "zh");
      localStorage.setItem("anixops.terms-accepted", "999.0.0");` });
    await page.goto(url);
    const agree = page.locator(".terms-overlay button.primary");
    if (await agree.count()) await agree.first().click();
    await page.locator(".nav-item", { hasText: "组网" }).first().click();
    const detail = page.getByRole("complementary", { name: "成员详情" });
    for (const expected of expectations.members) {
      const button = page.getByRole("row").filter({ hasText: expected.address })
        .getByRole("button", { name: expected.name, exact: true });
      await button.click();
      await verifyCopy(page, detail, "拷贝内网域名", expected.domain);
      await verifyCopy(page, detail, "拷贝虚拟地址", expected.address);
      for (const [kind, text] of Object.entries(warningTexts)) {
        assert.equal(await detail.getByText(text, { exact: true }).count(), Number(Boolean(expected[kind])), `${expected.name}: ${kind}`);
      }
      const cross = detail.getByText(/也应答这个名字/);
      assert.equal(await cross.count(), Number(Boolean(expected.otherNetworks?.length)), `${expected.name}: other networks`);
      for (const network of expected.otherNetworks ?? []) assert.ok((await cross.textContent()).includes(network));
      console.log(`${expected.name} (${expected.address}): domain ${expected.domain ?? "absent"}, clipboard and name warnings verified`);
    }
    if (expectations.own) {
      await page.getByRole("radio", { name: "本机", exact: true }).click();
      await verifyCopy(page, page, "拷贝内网域名", expectations.own.domain);
      await verifyCopy(page, page, "拷贝虚拟地址", expectations.own.address);
    }
  } finally {
    await browser.close();
  }
}

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
