// Compares two directories of screenshots (before and after) shot by shot, writes a report and fails when a
// change is not acknowledged in the pull request description (see ack.cjs).
//
//   node diff.cjs --before DIR --after DIR --report DIR [--label NAME]
//
// The description comes from the PR_BODY environment variable. Where the pictures come from does not matter: the
// drawn desktop interface, the macOS app, an iOS simulator or real windows at several scale factors all feed the
// same comparison, as long as both sides were taken the same way. Needs `pngjs` and `pixelmatch` resolvable.
const fs = require("node:fs");
const path = require("node:path");
const { PNG } = require("pngjs");
const pixelmatch = require("pixelmatch");
const { review } = require("./ack.cjs");

function options(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    if (!argv[index].startsWith("--") || argv[index + 1] === undefined) throw new Error(`bad argument ${argv[index]}`);
    values[argv[index].slice(2)] = argv[index + 1];
  }
  for (const name of ["before", "after", "report"]) if (!values[name]) throw new Error(`--${name} is required`);
  return values;
}

function shots(directory) {
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory).filter((file) => file.endsWith(".png")).map((file) => file.slice(0, -4)).sort();
}

/**
 * How one shot differs: "same", "added", "removed", "resized" or "changed", with the number of differing pixels
 * for the last. pixelmatch's threshold ignores colour differences too small to see; both sides come from the same
 * machine and browser, so any pixel it does report is a real difference.
 */
function compare(name, before, after, report) {
  const beforeFile = path.join(before, `${name}.png`);
  const afterFile = path.join(after, `${name}.png`);
  const hasBefore = fs.existsSync(beforeFile);
  const hasAfter = fs.existsSync(afterFile);
  const keep = (file, as) => {
    fs.mkdirSync(path.join(report, name), { recursive: true });
    fs.copyFileSync(file, path.join(report, name, as));
  };
  if (!hasBefore) {
    keep(afterFile, "after.png");
    return { name, state: "added" };
  }
  if (!hasAfter) {
    keep(beforeFile, "before.png");
    return { name, state: "removed" };
  }
  const a = PNG.sync.read(fs.readFileSync(beforeFile));
  const b = PNG.sync.read(fs.readFileSync(afterFile));
  if (a.width !== b.width || a.height !== b.height) {
    keep(beforeFile, "before.png");
    keep(afterFile, "after.png");
    return { name, state: "resized", detail: `${a.width}×${a.height} → ${b.width}×${b.height}` };
  }
  const picture = new PNG({ width: a.width, height: a.height });
  const pixels = pixelmatch(a.data, b.data, picture.data, a.width, a.height, { threshold: 0.1, includeAA: true });
  if (pixels === 0) return { name, state: "same" };
  keep(beforeFile, "before.png");
  keep(afterFile, "after.png");
  fs.writeFileSync(path.join(report, name, "diff.png"), PNG.sync.write(picture));
  const share = (pixels / (a.width * a.height)) * 100;
  return { name, state: "changed", pixels, detail: `${pixels} px (${share < 0.01 ? "<0.01" : share.toFixed(2)}%)` };
}

const stateText = { added: "新增", removed: "删除", resized: "尺寸变了", changed: "有变化" };

function main() {
  const { before, after, report, label = "screenshots" } = options(process.argv.slice(2));
  fs.rmSync(report, { recursive: true, force: true });
  fs.mkdirSync(report, { recursive: true });
  const names = [...new Set([...shots(before), ...shots(after)])].sort();
  if (shots(after).length === 0) throw new Error(`no screenshots in ${after}`);
  const results = names.map((name) => compare(name, before, after, report));
  const changed = results.filter((result) => result.state !== "same");
  const { covered, uncovered, unused } = review(changed.map((result) => result.name), process.env.PR_BODY);

  const lines = [`## 界面截图对比：${label}`, ""];
  if (changed.length === 0) {
    lines.push(`${names.length} 张截图与基线完全一致。`);
  } else {
    lines.push(`${names.length} 张截图里有 ${changed.length} 张与基线不同。每张的前后图和差异图在本次运行的截图产物的 \`report/\` 目录里。`, "");
    lines.push("| 截图 | 变化 | 详情 | 已在 PR 描述里确认 |", "|---|---|---|---|");
    for (const result of changed) {
      lines.push(`| \`${result.name}\` | ${stateText[result.state]} | ${result.detail ?? ""} | ${covered.includes(result.name) ? "是" : "**否**"} |`);
    }
  }
  if (uncovered.length > 0) {
    lines.push("", "**有未确认的界面变化。** 看过前后图、确认是想要的变化后，在 PR 描述里加一行（可以用 `*` 通配）：", "", "```", `界面变化：${uncovered.join(", ")}`, "```");
  }
  if (unused.length > 0) {
    lines.push("", `PR 描述里确认了但没有变化的截图：${unused.map((name) => `\`${name}\``).join("、")}。如果本该有变化，请检查改动是否生效。`);
  }
  const text = `${lines.join("\n")}\n`;
  fs.writeFileSync(path.join(report, "summary.md"), text);
  fs.writeFileSync(path.join(report, "summary.json"), `${JSON.stringify({ label, total: names.length, results, covered, uncovered, unused }, null, 2)}\n`);
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, text);
  process.stdout.write(text);
  if (uncovered.length > 0) {
    console.error(`::error::${uncovered.length} 张截图的变化没有在 PR 描述里确认：${uncovered.join(", ")}`);
    process.exit(1);
  }
}

try {
  main();
} catch (error) {
  console.error(`::error::${error.message}`);
  process.exit(2);
}
