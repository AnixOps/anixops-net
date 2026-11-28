// Draws the built interface (dist/) page by page with the demo backend in mock.js and saves one PNG per shot.
//   node render.cjs <dist directory> <output directory> [shot name ...]
//   UI_AUDIT=<report.json> node render.cjs ...   also run audit.cjs on every shot (docs/ui-ux.md, 自动检查) and
//                                                 fail on any finding; the report lists findings and measurements.
// Used by CI (.github/workflows/ui-screenshots.yml) to compare a pull request with its base, and by hand to look
// at a build. Needs `playwright` resolvable (CI installs a pinned version and sets NODE_PATH).
//
// The clock is frozen at a fixed time and advanced by hand, motion is off and nothing here is random, so the same
// build always gives the same pixels. Keep it that way: a shot that flickers makes every comparison useless.
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { chromium } = require("playwright");

const AUDIT = process.env.UI_AUDIT;
const { audit } = AUDIT ? require("./audit.cjs") : { audit: null };

const [dist, out, ...only] = process.argv.slice(2);
if (!dist || !out) {
  console.error("usage: node render.cjs <dist directory> <output directory> [shot name ...]");
  process.exit(2);
}

const CLOCK = new Date("2026-01-15T10:30:00+08:00");
const WIDE = { width: 1440, height: 900 };
const LAPTOP = { width: 1280, height: 800 };
const SMALL = { width: 960, height: 640 };

const row = (page, index) => page.locator('[role="row"][id^="request-"]').nth(index);
const tab = (page, name) => page.getByRole("tab", { name, exact: true });
const radio = (page, name) => page.getByRole("radio", { name });
const toggle = (page, name) => page.getByRole("switch", { name }).or(page.getByRole("checkbox", { name })).first();
/** Opens a pop-up (menu or list) and waits until it is on screen. */
const open = async (page, locator, popup) => { await locator.first().click(); await page.locator(popup).first().waitFor({ state: "visible" }); };

async function verifyMeshNames(page) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  const detail = page.getByRole("complementary", { name: "成员详情" });
  const camera = page.getByRole("row", { name: /camera/ });
  const printer = page.getByRole("row", { name: /printer/ });
  await camera.getByText("camera.uk.mesh.internal", { exact: true }).waitFor();
  await printer.getByText("192.168.10.14", { exact: true }).waitFor();
  if (await printer.getByRole("button").count()) throw new Error("address-only mesh child offered a copy action");

  for (const [container, label, text] of [
    [camera, "拷贝内网域名", "camera.uk.mesh.internal"],
    [detail, "拷贝内网域名", "nas.uk.mesh.internal"],
    [detail, "拷贝虚拟地址", "10.144.0.5"],
  ]) {
    const copy = container.getByRole("button", { name: label, exact: true });
    await copy.click();
    await copy.locator('[role="status"]').filter({ hasText: "已拷贝" }).waitFor({ state: "attached" });
    const actual = await page.evaluate(() => navigator.clipboard.readText());
    if (actual !== text) throw new Error(`mesh copy: expected ${text}, got ${actual}`);
  }

  const duplicate = "本地区有同名成员。";
  const crossNetwork = "网络 Lab 也应答这个名字，域名解析会返回全部地址。";
  await detail.getByText(crossNetwork, { exact: true }).waitFor();
  if (await detail.getByText(duplicate, { exact: true }).count()) throw new Error("cross-network duplicate showed a same-region warning");
  await page.getByRole("row", { name: /laptop.*重名/ }).getByRole("button", { name: "laptop" }).click();
  await detail.getByText(duplicate, { exact: true }).waitFor();
  if (await detail.getByText(crossNetwork, { exact: true }).count()) throw new Error("legacy duplicate showed a cross-network warning");
}

/** name, mode, theme, sidebar item, window size, and what to do on the page before the picture. */
const shots = [
  { name: "debug-requests-light", mode: "debug", theme: "light", nav: "请求", size: WIDE, steps: async (page) => { await row(page, 4).click(); } },
  { name: "debug-requests-dark", mode: "debug", theme: "dark", nav: "请求", size: WIDE, steps: async (page) => { await row(page, 4).click(); } },
  { name: "debug-requests-laptop-light", mode: "debug", theme: "light", nav: "请求", size: LAPTOP, steps: async (page) => { await row(page, 4).click(); } },
  { name: "debug-requests-small-light", mode: "debug", theme: "light", nav: "请求", size: SMALL },
  { name: "debug-requests-bottom-light", mode: "debug", theme: "light", nav: "请求", size: LAPTOP, storage: { "anixops.inspector-dock": "bottom" }, steps: async (page) => { await row(page, 4).click(); } },
  { name: "debug-inspector-overview-light", mode: "debug", theme: "light", nav: "请求", size: WIDE, steps: async (page) => { await row(page, 4).click(); await tab(page, "概览").click(); } },
  { name: "debug-inspector-request-light", mode: "debug", theme: "light", nav: "请求", size: WIDE, steps: async (page) => { await row(page, 4).click(); await tab(page, "请求").click(); } },
  { name: "debug-inspector-pretty-light", mode: "debug", theme: "light", nav: "请求", size: WIDE, steps: async (page) => { await row(page, 4).click(); await radio(page, "格式化").click(); } },
  { name: "debug-inspector-timeline-light", mode: "debug", theme: "light", nav: "请求", size: WIDE, steps: async (page) => { await row(page, 4).click(); await tab(page, "时间线").click(); } },
  { name: "debug-rewrite-light", mode: "debug", theme: "light", nav: "改写", size: LAPTOP },
  { name: "debug-exit-light", mode: "debug", theme: "light", nav: "出口", size: LAPTOP },
  { name: "debug-settings-light", mode: "debug", theme: "light", nav: "设置", size: LAPTOP },
  { name: "home-light", mode: "proxy", theme: "light", nav: "概览", size: LAPTOP },
  { name: "home-dark", mode: "proxy", theme: "dark", nav: "概览", size: LAPTOP },
  { name: "proxies-light", mode: "proxy", theme: "light", nav: "代理", size: LAPTOP },
  { name: "proxies-dark", mode: "proxy", theme: "dark", nav: "代理", size: LAPTOP },
  { name: "rules-light", mode: "proxy", theme: "light", nav: "规则", size: LAPTOP },
  { name: "extensions-light", mode: "proxy", theme: "light", nav: "扩展", size: LAPTOP },
  { name: "activity-light", mode: "proxy", theme: "light", nav: "活动", size: LAPTOP },
  { name: "settings-light", mode: "proxy", theme: "light", nav: "设置", size: LAPTOP },
  // Every page in the dark theme too: most layering mistakes only show there.
  { name: "rules-dark", mode: "proxy", theme: "dark", nav: "规则", size: LAPTOP },
  { name: "extensions-dark", mode: "proxy", theme: "dark", nav: "扩展", size: LAPTOP },
  { name: "activity-dark", mode: "proxy", theme: "dark", nav: "活动", size: LAPTOP },
  { name: "settings-dark", mode: "proxy", theme: "dark", nav: "设置", size: LAPTOP },
  // The settings sub-pages and the folded plugin list, reached the way a person does.
  { name: "settings-advanced-light", mode: "proxy", theme: "light", nav: "设置", size: LAPTOP, steps: async (page) => { await page.getByRole("button", { name: "高级" }).click(); } },
  { name: "settings-advanced-dark", mode: "proxy", theme: "dark", nav: "设置", size: LAPTOP, steps: async (page) => { await page.getByRole("button", { name: "高级" }).click(); } },
  { name: "settings-privacy-light", mode: "proxy", theme: "light", nav: "设置", size: LAPTOP, steps: async (page) => { await page.getByRole("button", { name: "隐私与诊断" }).click(); } },
  { name: "settings-wifi-light", mode: "proxy", theme: "light", nav: "设置", size: LAPTOP, steps: async (page) => { await page.getByRole("button", { name: "Wi-Fi 规则" }).click(); } },
  { name: "settings-advanced-en-dark", mode: "proxy", theme: "dark", language: "en", nav: "Settings", size: SMALL, steps: async (page) => { await page.getByRole("button", { name: "Advanced" }).click(); } },
  { name: "extensions-expanded-light", mode: "proxy", theme: "light", nav: "扩展", size: LAPTOP, steps: async (page) => { await page.getByRole("button", { name: "显示全部 7 个插件" }).click(); } },
  { name: "debug-rewrite-dark", mode: "debug", theme: "dark", nav: "改写", size: LAPTOP },
  { name: "debug-exit-dark", mode: "debug", theme: "dark", nav: "出口", size: LAPTOP },
  { name: "debug-settings-dark", mode: "debug", theme: "dark", nav: "设置", size: LAPTOP },
  // English: longer words find truncation and wrapping the Chinese layout never shows.
  { name: "home-en-light", mode: "proxy", theme: "light", language: "en", nav: "Overview", size: LAPTOP },
  { name: "proxies-en-dark", mode: "proxy", theme: "dark", language: "en", nav: "Proxies", size: LAPTOP },
  { name: "rules-en-light", mode: "proxy", theme: "light", language: "en", nav: "Rules", size: LAPTOP },
  { name: "extensions-en-light", mode: "proxy", theme: "light", language: "en", nav: "Extensions", size: SMALL },
  { name: "settings-en-dark", mode: "proxy", theme: "dark", language: "en", nav: "Settings", size: SMALL },
  { name: "debug-requests-en-light", mode: "debug", theme: "light", language: "en", nav: "Request", size: LAPTOP, steps: async (page) => { await row(page, 4).click(); } },
  // Dialogs, menus and pop-up lists, in both themes: an overlay has to stand apart from the window under it.
  { name: "terms-first-light", mode: "proxy", theme: "light", storage: { "anixops.terms-accepted": null }, dialog: true, size: LAPTOP },
  { name: "terms-first-dark", mode: "proxy", theme: "dark", storage: { "anixops.terms-accepted": null }, dialog: true, size: LAPTOP },
  { name: "terms-viewer-light", mode: "proxy", theme: "light", nav: "设置", size: LAPTOP, steps: async (page) => { await page.getByRole("button", { name: "查看用户协议", exact: true }).click(); } },
  { name: "terms-viewer-dark", mode: "proxy", theme: "dark", nav: "设置", size: LAPTOP, steps: async (page) => { await page.getByRole("button", { name: "查看用户协议", exact: true }).click(); } },
  { name: "decryption-consent-light", mode: "proxy", theme: "light", nav: "扩展", size: LAPTOP, storage: { "anixops.https-decryption-consent": null, "anixops.mock-decryption": "off" }, steps: async (page) => { await toggle(page, "解密 HTTPS 流量").click(); } },
  { name: "decryption-consent-dark", mode: "proxy", theme: "dark", nav: "扩展", size: LAPTOP, storage: { "anixops.https-decryption-consent": null, "anixops.mock-decryption": "off" }, steps: async (page) => { await toggle(page, "解密 HTTPS 流量").click(); } },
  { name: "debug-menu-capture-light", mode: "debug", theme: "light", nav: "请求", size: LAPTOP, steps: async (page) => { await open(page, page.getByRole("button", { name: "更多抓包操作" }), '[role="menu"]'); } },
  { name: "debug-menu-capture-dark", mode: "debug", theme: "dark", nav: "请求", size: LAPTOP, steps: async (page) => { await open(page, page.getByRole("button", { name: "更多抓包操作" }), '[role="menu"]'); } },
  { name: "select-rules-light", mode: "proxy", theme: "light", nav: "规则", size: LAPTOP, steps: async (page) => { await open(page, page.locator(".select-trigger"), '[role="listbox"]'); } },
  { name: "select-rules-dark", mode: "proxy", theme: "dark", nav: "规则", size: LAPTOP, steps: async (page) => { await open(page, page.locator(".select-trigger"), '[role="listbox"]'); } },
  // 组网: the member list with a member's details, the subnets and the cross-region links, in both themes and in English.
  { name: "mesh-members-light", mode: "proxy", theme: "light", nav: "组网", size: WIDE, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await page.getByRole("button", { name: "nas", exact: true }).click(); }, verify: verifyMeshNames },
  { name: "mesh-members-dark", mode: "proxy", theme: "dark", nav: "组网", size: WIDE, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await page.getByRole("row", { name: /nas/ }).first().click(); } },
  { name: "mesh-subnets-light", mode: "proxy", theme: "light", nav: "组网", size: LAPTOP, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await radio(page, "子网").click(); } },
  { name: "mesh-paths-dark", mode: "proxy", theme: "dark", nav: "组网", size: LAPTOP, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await radio(page, "路径").click(); } },
  { name: "mesh-self-light", mode: "proxy", theme: "light", nav: "组网", size: LAPTOP, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await radio(page, "本机").click(); } },
  { name: "mesh-self-dark", mode: "proxy", theme: "dark", nav: "组网", size: LAPTOP, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await radio(page, "本机").click(); } },
  { name: "mesh-path-editor-light", mode: "proxy", theme: "light", nav: "组网", size: WIDE, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await radio(page, "路径").click(); await page.getByRole("button", { name: "添加路径" }).click(); } },
  { name: "mesh-add-credential-light", mode: "proxy", theme: "light", nav: "组网", size: LAPTOP, storage: { "anixops.mock-mesh": "on" }, steps: async (page) => { await open(page, page.getByRole("button", { name: "更多" }), '[role="menu"]'); await page.getByRole("menuitem", { name: "添加网络" }).click(); await radio(page, "入网凭据").click(); } },
  { name: "mesh-members-en-light", mode: "proxy", theme: "light", language: "en", nav: "Mesh", size: SMALL, storage: { "anixops.mock-mesh": "on" } },
  {
    name: "mesh-unmanaged-light", mode: "proxy", theme: "light", nav: "组网", size: WIDE,
    storage: { "anixops.mock-mesh": "on", "anixops.mock-mesh-unmanaged": "on" },
    verify: async (page) => {
      await page.getByText("2 台成员的地址未受管，地址可能变化。", { exact: true }).waitFor();
      await page.locator(".nav-item").getByText("组网有提示", { exact: true }).waitFor({ state: "attached" });
    },
  },
];

const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2", ".svg": "image/svg+xml", ".png": "image/png", ".ico": "image/x-icon" };

function serve(root) {
  const base = path.resolve(root);
  const server = http.createServer((request, response) => {
    const name = decodeURIComponent(request.url.split("?")[0]);
    const file = path.resolve(base, `.${name === "/" ? "/index.html" : name}`);
    if (!file.startsWith(base + path.sep) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      response.writeHead(404);
      response.end();
      return;
    }
    response.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
    response.end(fs.readFileSync(file));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

async function shoot(browser, url, shot) {
  const context = await browser.newContext({
    viewport: shot.size, deviceScaleFactor: 1, colorScheme: shot.theme, locale: "zh-CN", timezoneId: "Asia/Shanghai", reducedMotion: "reduce",
  });
  // The clock goes in first: mock.js reads the time when it loads and must see the frozen one.
  // Installed a little earlier, because a running clock cannot be paused at a time already gone by.
  await context.clock.install({ time: new Date(CLOCK.getTime() - 60_000) });
  await context.addInitScript({ path: path.join(__dirname, "mock.js") });
  await context.addInitScript(([mode, theme, language, extra]) => {
    const values = {
      "anixops.app-mode": mode, "anixops.appearance": theme, "anixops.language": language, "anixops.terms-accepted": "999.0.0",
      "anixops.https-decryption-consent": "2026-01-01T00:00:00.000Z", ...extra,
    };
    // null: the key is not stored at all (a first start, a consent not given yet).
    for (const [key, value] of Object.entries(values)) {
      if (value === null) window.localStorage.removeItem(key);
      else window.localStorage.setItem(key, value);
    }
  }, [shot.mode, shot.theme, shot.language ?? "zh", shot.storage ?? {}]);
  const page = await context.newPage();
  // A control that is not there (an older build) should fail the shot quickly, not hold up the rest.
  page.setDefaultTimeout(5000);
  const problems = [];
  page.on("pageerror", (error) => problems.push(String(error)));
  await page.clock.pauseAt(CLOCK);
  await page.goto(url);
  await page.clock.runFor(1000);
  // A dialog shot is the first-start dialog itself: it stays open, over whatever the window shows behind it.
  if (shot.dialog) {
    await page.clock.runFor(8000);
  } else {
    // The stored acceptance is for another version of the terms: accept the ones this build shows.
    const agree = page.locator(".terms-overlay button.primary");
    if (await agree.count()) {
      await agree.first().click();
      await page.clock.runFor(300);
    }
    await page.locator(".nav-item", { hasText: shot.nav }).first().click();
    // Long enough for the pages that poll once a second to fill their lists and a few points of their charts.
    await page.clock.runFor(8000);
    // Entry invariant (docs/ui-ux.md, 入口不变量 1): every page of either mode keeps the connection switch, once, at
    // the foot of the sidebar. Debug mode once lost its only switch on iOS when its overview went away.
    const switches = await page.locator('.sidebar-foot [role="switch"]').count();
    if (switches !== 1) problems.push(`entry invariant: ${switches} connection switches at the foot of the sidebar, want 1`);
  }
  if (shot.steps) {
    await shot.steps(page);
    await page.clock.runFor(3000);
  }
  await page.evaluate(() => document.fonts.ready);
  const file = path.join(out, `${shot.name}.png`);
  await page.screenshot({ path: file, animations: "disabled", caret: "hide" });
  let measures = [];
  if (audit) {
    const checked = await audit(page, file);
    problems.push(...checked.findings);
    measures = checked.measures;
    if (shot.verify) await shot.verify(page);
  }
  await context.close();
  return { problems, measures };
}

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const wanted = only.length > 0 ? shots.filter((shot) => only.includes(shot.name)) : shots;
  const server = await serve(dist);
  const url = `http://127.0.0.1:${server.address().port}/`;
  const browser = await chromium.launch();
  let failed = false;
  const report = {};
  for (const shot of wanted) {
    try {
      const { problems, measures } = await shoot(browser, url, shot);
      report[shot.name] = { problems, measures };
      // A page that throws is a finding, not a reason to have no picture of the others.
      if (problems.length > 0) {
        failed = true;
        console.error(`${shot.name}: page errors:\n  ${problems.join("\n  ")}`);
      } else {
        console.log(`${shot.name}: ok`);
      }
    } catch (error) {
      failed = true;
      console.error(`${shot.name}: ${error.message.split("\n")[0]}`);
    }
  }
  await browser.close();
  server.close();
  if (AUDIT) fs.writeFileSync(AUDIT, `${JSON.stringify(report, null, 2)}\n`);
  process.exit(failed ? 1 : 0);
})();
