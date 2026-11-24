// Demo backend for screenshots: stands in for the Tauri side (window.__TAURI_INTERNALS__.invoke) with fixed data,
// so the built interface can be drawn in a plain browser. Loaded as an init script by render.cjs before the app.
//
// Everything here is a pure function of the (frozen) clock: no randomness and no call counters, so two renders
// of the same build give the same pixels. When bridge.ts gains a command a page needs, add its answer here.
(() => {
  // render.cjs lets the clock run for the minute before the frozen time while the page loads, so how long the
  // load takes would move every timestamp below by a fraction of a second (and now and then across a second).
  // The whole minute is the same in every run.
  const start = Math.floor(Date.now() / 60_000) * 60_000;
  const utf8 = (text) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));
  const hosts = ["api.example.com", "cdn.example.com", "events.example.com", "auth.example.com", "img.example.net", "telemetry.example.org"];
  // path, method, response type, request body kind
  const routes = [
    ["/v2/feed?cursor=eyJwIjoyfQ&limit=20", "GET", "application/json", null],
    ["/v2/users/me", "GET", "application/json", null],
    ["/assets/app.9f3c1.js", "GET", "text/javascript", null],
    ["/v1/track", "POST", "", "json"],
    ["/oauth/token", "POST", "application/json", "form"],
    ["/v2/orders/18422/items", "GET", "application/json", null],
    ["/media/cover.png", "GET", "image/png", null],
    ["/v2/search?q=trail+shoes&page=2&sort=price", "GET", "application/json", null],
    ["/graphql", "POST", "application/json", "json"],
    ["/v2/cart", "PUT", "application/json", "json"],
    ["/v2/cart/items/7", "DELETE", "", null],
  ];
  const statuses = [200, 200, 200, 204, 200, 304, 200, 201, 500, 404, 200, 413, 302, 200, 401];
  const perConnection = new Map();
  const requests = [];
  const COUNT = 180;
  for (let i = 0; i < COUNT; i += 1) {
    const host = hosts[i % hosts.length];
    const [path, method, type, sends] = routes[(i * 7) % routes.length];
    const connection = 100 + (i % 23);
    const index = perConnection.get(connection) ?? 0;
    perConnection.set(connection, index + 1);
    const started = start - (COUNT - i) * 1400;
    const pending = i === COUNT - 2;
    const empty = type === "";
    requests.push({
      seq: i + 1, connection_id: connection, index, method, url: `https://${host}${path}`, host,
      status: pending ? null : empty ? 204 : type.startsWith("image") || type === "text/javascript" ? 200 : statuses[(i * 3) % statuses.length],
      note: null, http_version: i % 3 ? "HTTP/2" : "HTTP/1.1",
      started_at_ms: started, ended_at_ms: pending ? null : started + 20 + ((i * 37) % 900),
      request_size: sends ? 180 + ((i * 91) % 900) : 0, response_size: pending || empty ? 0 : 120 + ((i * 7919) % 480000),
      response_content_type: type || null, sends,
    });
  }

  // Written as text, not built from an object: the ids are beyond what a JavaScript number holds, and the tree
  // has to show them exactly as a server would send them.
  const feedText = '{"items":['
    + '{"id":9007199254740993,"title":"Trail shoes","price":129.00,"currency":"EUR","in_stock":true,"tags":["sale","new"],"seller":{"id":4821,"name":"North Ridge","rating":4.7}},'
    + '{"id":9007199254740995,"title":"Rain jacket","price":89.50,"currency":"EUR","in_stock":false,"tags":[],"seller":{"id":1177,"name":"Harbor & Co","rating":4.2}},'
    + '{"id":9007199254740997,"title":"Wool socks (3 pairs)","price":18,"currency":"EUR","in_stock":true,"tags":["bundle"],"seller":null}'
    + '],"next":"eyJwIjozfQ","total":248,"took_ms":12}';
  const script = "export function boot(root) {\n  const app = createApp(root);\n  app.mount();\n  return app;\n}\n";
  let picture = null;
  const pictureBase64 = () => {
    if (picture) return picture;
    const canvas = document.createElement("canvas");
    canvas.width = 320;
    canvas.height = 200;
    const pen = canvas.getContext("2d");
    const sky = pen.createLinearGradient(0, 0, 0, 200);
    sky.addColorStop(0, "#4f5be8");
    sky.addColorStop(1, "#0ea5e9");
    pen.fillStyle = sky;
    pen.fillRect(0, 0, 320, 200);
    pen.fillStyle = "#0f172a";
    pen.beginPath();
    pen.moveTo(0, 200); pen.lineTo(90, 90); pen.lineTo(150, 150); pen.lineTo(220, 60); pen.lineTo(320, 200);
    pen.fill();
    pen.fillStyle = "#f59e0b";
    pen.beginPath();
    pen.arc(256, 48, 20, 0, Math.PI * 2);
    pen.fill();
    picture = canvas.toDataURL("image/png").split(",")[1];
    return picture;
  };
  const body = (text) => ({ size: new TextEncoder().encode(text).length, truncated: false, data_base64: utf8(text) });
  const none = { size: 0, truncated: false, data_base64: "" };
  const exchange = (row) => {
    const type = row.response_content_type;
    let response = none;
    if (row.status == null) response = none;
    else if (type === "application/json") response = body(row.status >= 400 ? JSON.stringify({ error: { code: row.status, message: "request failed", request_id: "7f3a9c1e-22b4" } }) : feedText);
    else if (type === "text/javascript") response = body(script);
    else if (type === "image/png") {
      const data = pictureBase64();
      response = { size: Math.floor(data.length * 3 / 4), truncated: false, data_base64: data };
    }
    const sent = row.sends === "form" ? body("grant_type=refresh_token&refresh_token=def50200a1b2&client_id=demo-app&scope=read+write")
      : row.sends === "json" ? body(JSON.stringify({ event: "screen_view", screen: "feed", properties: { items: 20, source: "pull_to_refresh" } }))
      : none;
    const sentType = row.sends === "form" ? "application/x-www-form-urlencoded" : "application/json";
    return {
      started_at_ms: row.started_at_ms, ended_at_ms: row.ended_at_ms, method: row.method, url: row.url, status: row.status, note: row.note,
      request: {
        headers: [[":authority", row.host], ["user-agent", "DemoApp/4.2 (iPhone; iOS 18.1)"], ["accept", "application/json"], ["authorization", "Bearer eyJhbGciOiJIUzI1NiJ9.demo.sig"], ["accept-language", "zh-CN,zh;q=0.9"], ["x-request-id", "7f3a9c1e-22b4"], ...(row.sends ? [["content-type", sentType]] : [])],
        body: sent,
      },
      response: {
        headers: row.status == null ? [] : [...(type ? [["content-type", type === "application/json" ? "application/json; charset=utf-8" : type]] : []), ["cache-control", "private, max-age=0"], ["server", "envoy"], ["x-trace-id", "b8d1e0aa41"], ["content-length", String(response.size)]],
        body: response,
      },
    };
  };

  const nodeNames = ["香港 01", "香港 02", "日本 东京 01", "日本 大阪 02", "新加坡 01", "美国 洛杉矶 01", "美国 圣何塞 02", "台湾 01", "韩国 首尔 01", "德国 法兰克福 01", "英国 伦敦 01", "香港 IEPL 03"];
  const nodes = nodeNames.map((label, i) => ({ id: `n${i}`, label, protocol: ["vless", "ss2022", "trojan", "anx"][i % 4], selected: i === 0, group: "AnixOps", editable: true }));
  const conn = (i, ended) => ({
    id: 900 + i, network: i % 6 ? "tcp" : "udp",
    host: ["youtube.com", "github.com", "api.openai.com", "icloud.com", "telegram.org", "baidu.com", "rr3.googlevideo.com"][i % 7],
    process: ["chrome.exe", "Code.exe", "Telegram.exe", "msedge.exe"][i % 4], destination: `142.250.${i}.14:443`,
    rule: i % 5 ? "MATCH" : "DOMAIN-SUFFIX,baidu.com", chain: [i % 5 ? "香港 01" : "DIRECT"], outbound: i % 5 ? "香港 01" : "DIRECT",
    started_at_ms: start - i * 9000, ended_at_ms: ended ? start - i * 1000 : null, upload: 1200 * (i + 1), download: 880000 * (i + 1), error: null, captured: i % 3 === 0,
  });
  const traffic = () => {
    const t = Math.floor((Date.now() - start) / 1000);
    return { upload_total: 82.4e6 + t * 2e5, download_total: 1.8e9 + t * 3e6, upload_rate: Math.round(150e3 + 60e3 * Math.sin(t / 3)), download_rate: Math.round(2.6e6 + 1.4e6 * Math.sin(t / 5)), active: 27 };
  };
  const stamp = "2026-01-15 08:30";
  // `anixops.mock-decryption` = "off": decryption starts off, so turning it on asks for consent (decryption-consent-* shots).
  const snapshot = {
    platform: "windows", connection: "Connected", connectionLabel: "已连接", core: { label: "运行中", tone: "success" }, proxy: { label: "已启用", tone: "success" },
    selectedNode: "香港 01", subscription: "AnixOps", subscriptionLastUpdated: stamp, subscriptionRefreshResult: "success",
    subscriptionAddedNodeCount: 0, subscriptionRemovedNodeCount: 0, subscriptionChangedNodeCount: 0,
    startAfterLogin: true, autoConnect: true, autoRecoverCore: true, autoSubscriptionRefresh: true, autoSelectFastestNode: false,
    tunModeEnabled: false, scriptRuntimeConfigured: true, darkTheme: false, routingMode: "rule", dnsPreset: "encrypted", blockQuic: false, directEgress: false, embeddedEngine: true,
    // Read when the page asks, not when this file loads: render.cjs stores the shot's values after it.
    get httpsDecryptionEnabled() { return window.localStorage.getItem("anixops.mock-decryption") !== "off"; },
  };
  const hours = Array.from({ length: 24 }, (_, i) => ({
    start: Math.floor(start / 3600e3) * 3600 - (23 - i) * 3600, upload: 2e6 * (1 + (i % 5)), download: 60e6 * (1 + ((i * 3) % 7)),
    hosts: { "youtube.com": 4e7 * (i % 4), "github.com": 1e7 }, policies: { "香港 01": 5e7, DIRECT: 1e7 },
  }));
  const control = (request) => {
    switch (request.cmd) {
      case "traffic": return traffic();
      case "connections": return { traffic: traffic(), active: Array.from({ length: 14 }, (_, i) => conn(i, false)), closed: Array.from({ length: 10 }, (_, i) => conn(i + 20, true)) };
      case "capture": case "set_capture": case "clear_capture": return { enabled: true, max_total_bytes: 256e6, used_bytes: 38.4e6, connections: 23 };
      case "capture_requests": return { requests: requests.filter((row) => row.seq > (request.since_seq || 0)).map(({ sends, ...row }) => row), last_seq: COUNT };
      case "capture_connection": {
        const rows = requests.filter((row) => row.connection_id === request.id).sort((a, b) => a.index - b.index);
        if (rows.length === 0) return { capture: null };
        return { capture: { id: request.id, network: "tcp", host: rows[0].host, destination: "203.0.113.20:443", started_at_ms: rows[0].started_at_ms, ended_at_ms: null, decrypted: true, upload: { seen: 4000, captured: 4000, truncated: false }, download: { seen: 90000, captured: 90000, truncated: false }, chunks: [], http: rows.map(exchange) } };
      }
      case "rule_sets": return { rule_sets: [{ name: "geosite-cn", source: "remote", entries: 71234, updated_at_ms: start - 3600e3 }, { name: "geosite-google", source: "remote", entries: 6012, updated_at_ms: start - 86400e3, skipped: { SCRIPT: 3 } }], missing: ["asn-13335"] };
      case "rule_hits": case "clear_rule_hits": return { generation: 1, rules: [
        { rule: "DOMAIN-SUFFIX,example.com", target: "节点选择", hits: 412, last_hit_at_ms: start - 2e3 },
        { rule: "GEOIP,CN", target: "DIRECT", hits: 1288, last_hit_at_ms: start - 1e3 },
        { rule: "IP-ASN,13335", target: "节点选择", hits: 0 },
      ], final: { rule: "FINAL", target: "节点选择", hits: 96, last_hit_at_ms: start - 4e3 } };
      case "test_route": return { mode: "rule", test: { rule_index: 0, rule: "DOMAIN-SUFFIX,example.com", chain: ["节点选择"], outbound: "香港 01" } };
      case "mitm_hosts": case "clear_mitm_hosts": return { enabled: true, hosts: [
        { host: "auth.example.com", decrypted: 0, failed: 4, last_failure: "client_rejected_certificate", last_failure_at_ms: start - 30e3, failures: { client_rejected_certificate: 4 } },
        { host: "api.example.com", decrypted: 128, failed: 0, last_decrypted_at_ms: start - 5e3 },
      ] };
      case "lan_share": return { running: false, port: null, addresses: ["192.168.1.23"], url: null, page_host: "anixops.local", https_decryption: true, certificate: true, credentials: false, accepted: 0, rejected: 0, active: 0, pages: 0 };
      default: return {};
    }
  };
  const handlers = {
    runtime_snapshot: () => snapshot,
    list_nodes: () => nodes,
    list_native_groups: () => [
      { tag: "节点选择", groupType: "selector", selected: "香港 01", outbounds: nodeNames.slice(0, 6) },
      { tag: "自动选择", groupType: "urltest", selected: "日本 东京 01", outbounds: nodeNames.slice(0, 8) },
      { tag: "流媒体", groupType: "selector", selected: "新加坡 01", outbounds: nodeNames.slice(2, 7) },
    ],
    list_subscriptions: () => [
      { id: "s1", location: "https://sub.example.com/api/v1/client/subscribe?token=demo", selected: true, lastSuccessfulUpdate: stamp, result: "success", addedNodeCount: 0, removedNodeCount: 0, changedNodeCount: 0, name: "AnixOps", nodeCount: 12, upload: 5.5e9, download: 58e9, total: 186.26e9, expire: Math.floor(start / 1000) + 23 * 86400, updateIntervalHours: 12, refreshable: true },
      { id: "s2", location: "https://backup.example.net/sub", selected: false, result: "success", addedNodeCount: 0, removedNodeCount: 0, changedNodeCount: 0, name: "备用线路", nodeCount: 5, updateIntervalHours: 24, refreshable: true },
    ],
    runtime_control: (args) => control(args.request),
    test_node_delays: (args) => (args.nodeIds ?? []).map((id) => {
      const i = Number(id.slice(1)) || 0;
      const delay = [48, 52, 71, 88, 64, 148, 162, 59, 77, 231, 244, null][i % 12];
      return { nodeId: id, delayMs: delay, error: delay == null ? "timeout" : null };
    }),
    network_probe: () => ({ exitIp: "203.0.113.45", exitLocation: "HK", directMs: 23 }),
    traffic_stats: () => ({ hours }),
    app_logs: () => ["engine", "dns", "mitm", "subscription", "rules"].flatMap((module, i) => [0, 1, 2].map((j) => ({
      timeMs: start - (i * 3 + j) * 41000, module, level: ["info", "info", "warn", "error"][(i + j) % 4],
      message: ["outbound 香港 01 connected in 48 ms", "resolved api.example.com via DoH in 12 ms", "plugin reloaded: 3 rewrite rules, 1 script", "subscription AnixOps updated: 12 nodes", "handshake timeout to 198.51.100.7:443"][(i + j) % 5],
    }))),
    take_app_links: () => [],
    notification_settings: () => ({ scripts: true, connection: true, subscriptions: true, language: "zh-Hans" }),
    on_demand_settings: () => ({ enabled: true, wifi: [{ ssid: "Home", action: "disconnect" }, { ssid: "Office", action: "disconnect" }, { ssid: "Cafe", action: "connect" }, { ssid: "Library", action: "connect" }, { ssid: "Station", action: "disconnect" }, { ssid: "Hotel", action: "connect" }], otherWifi: "connect", cellular: "connect", other: "ignore" }),
    health_status: () => ({ crashReports: false, crashReportsKept: 0, crashUploadConfigured: false, integrity: "verified", coldStartMs: 412 }),
    list_debug_mappings: () => [
      { id: "m1", name: "订单接口返回 500", enabled: true, kind: "local", match: "prefix", pattern: "https://api.example.com/v2/orders", status: 500, headers: [], body: utf8('{"error":"boom"}'), target: "" },
      { id: "m2", name: "API 指到本机", enabled: false, kind: "remote", match: "prefix", pattern: "https://api.example.com/", status: 200, headers: [], body: "", target: "http://127.0.0.1:3000/" },
      { id: "m3", name: "用户接口超时", enabled: true, kind: "local", match: "prefix", pattern: "https://api.example.com/v2/users", status: 504, headers: [], body: "", target: "" },
      { id: "m4", name: "图片指到测试库", enabled: false, kind: "remote", match: "prefix", pattern: "https://img.example.com/", status: 200, headers: [], body: "", target: "http://127.0.0.1:3100/" },
      { id: "m5", name: "配置接口返回旧版", enabled: true, kind: "local", match: "prefix", pattern: "https://api.example.com/config", status: 200, headers: [], body: utf8('{"version":1}'), target: "" },
      { id: "m6", name: "支付回调指到本机", enabled: false, kind: "remote", match: "prefix", pattern: "https://pay.example.com/callback", status: 200, headers: [], body: "", target: "http://127.0.0.1:3200/" },
    ],
    list_decrypt_hosts: () => ["api.example.com", "*.example.net", "auth.example.com"],
    list_decrypt_domains: () => [
      { pattern: "api.example.com", exclude: false, enabled: true },
      { pattern: "*.example.net", exclude: false, enabled: true },
      { pattern: "auth.example.com", exclude: false, enabled: true },
      { pattern: "cdn.example.net", exclude: false, enabled: true },
      { pattern: "*.example.org", exclude: false, enabled: true },
      { pattern: "static.example.com", exclude: false, enabled: false },
      { pattern: "pay.example.net", exclude: true, enabled: true },
    ],
    capture_session: () => ({ protection: "dpapi", retention_days: 7, sessions: [{ session: "a1", name: "登录流程", saved_at_ms: start - 3600e3, expires_at_ms: start + 6 * 86400e3, size: 4.2e6, connections: 23, requests: COUNT }] }),
    mitm_root_status: () => ({ generated: true, trusted: true }),
    mitm_root_fingerprint: () => "9F:3C:1A:22:B4:7E:5D:08",
    list_mitm_plugins: () => [
      { id: "p1", name: "Bilibili 去广告", url: "https://example.com/bili.plugin", enabled: true, level: "full", hostnames: ["api.bilibili.com", "app.bilibili.com"], scriptCount: 3 },
      { id: "p2", name: "YouTube 去广告", url: "https://example.com/yt.plugin", enabled: true, level: "partial", hostnames: ["*.googlevideo.com"], scriptCount: 1 },
      // More than five, so the list shows folded with its "显示全部" row.
      { id: "p3", name: "微博 去广告", url: "https://example.com/weibo.plugin", enabled: true, level: "full", hostnames: ["api.weibo.cn"], scriptCount: 2 },
      { id: "p4", name: "知乎 净化", url: "https://example.com/zhihu.plugin", enabled: false, level: "full", hostnames: ["api.zhihu.com"], scriptCount: 1 },
      { id: "p5", name: "京东 比价", url: "https://example.com/jd.plugin", enabled: false, level: "partial", hostnames: ["api.m.jd.com"], scriptCount: 2 },
      { id: "p6", name: "网页 去重定向", url: "https://example.com/redirect.plugin", enabled: true, level: "full", hostnames: ["*.example.org"], scriptCount: 0 },
      { id: "p7", name: "天气 小组件", url: "https://example.com/weather.plugin", enabled: false, level: "full", hostnames: ["weather.example.com"], scriptCount: 1 },
    ],
    custom_dns: () => ({ servers: "", rules: "", hosts: "", fake_ip: false }),
    routing_warnings: () => ["2 rules of type SCRIPT were not imported: the client does not support them yet"],
    data_sources: () => [{ name: "geoip-us", url: "https://lists.example.org/geoip/us.txt" }],
    devices: () => [{ address: "192.168.1.30", name: "Apple TV" }],
    my_rules: () => ({ text: "DOMAIN-SUFFIX,example.com,PROXY\nDOMAIN-KEYWORD,ads,REJECT\n", count: 2 }),
    phone_qr_code: () => "",
    // Made-up credentials: the screenshots show them masked.
    lan_share_auth: () => ({ required: true, username: "anx-k7m2qp", password: "x4ve9tqh2nbs7wdk3m8r" }),
    regenerate_lan_share_auth: () => ({ required: true, username: "anx-r5t8wz", password: "c6hj3pq9vk2mx7ns4bfe" }),
    set_lan_share_auth_required: (args) => ({ required: Boolean(args && args.required), username: "anx-k7m2qp", password: "x4ve9tqh2nbs7wdk3m8r" }),
  };
  let callbacks = 0;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: (callback) => {
      callbacks += 1;
      window[`_${callbacks}`] = callback;
      return callbacks;
    },
    invoke: async (command, args) => {
      if (command.startsWith("plugin:")) return command.includes("listen") ? 1 : null;
      const handler = handlers[command];
      return handler ? handler(args ?? {}) : { message: "ok" };
    },
  };
})();
