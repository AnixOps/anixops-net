// Checks one drawn page for the interface problems a screenshot comparison cannot see (docs/ui-ux.md, 自动检查,
// 第二层). render.cjs calls it after the picture is taken, so nothing here changes the pixels. Three checks:
//
//   axe      AnixOps Design accessibility.md: axe with WCAG 2.2 A/AA, serious and critical findings = 0.
//            Colour contrast is the one that matters most here: it is computed on the real, layered colours.
//   clip     Text cut off without an ellipsis: a box that hides overflowing text, or text pushed out of the window.
//            Ellipsis truncation is a choice (N9) and is not a finding.
//   layer    A dialog, menu, pop-up list, tooltip or toast that does not stand apart from what is under it: neither
//            its surface nor its edge differs enough from the pixels just outside it (layout.md, Elevation).
//            Measured on the screenshot itself, in CIE L* (0 black, 100 white).
//
// Each finding is one line; the caller fails the shot when there is any. Needs axe-core and pngjs resolvable.
const fs = require("node:fs");
const { PNG } = require("pngjs");

const AXE = require.resolve("axe-core/axe.min.js");
const AXE_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

// An overlay passes when its surface differs from the surroundings by at least SURFACE, or its edge (the 3 px band
// on the boundary: hairline, highlight edge or the shadow's start) by at least EDGE. A light dialog on a frosted
// light window passes by its hairline and shadow; a #1C1C1E sheet on a #1C1C1E window fails both.
const SURFACE_DELTA_L = 4;
const EDGE_DELTA_L = 6;
const OVERLAYS = '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [role="tooltip"], .popover, .toast';

function srgbToLinear(c) {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}

function lightness(r, g, b) {
  const y = 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b);
  return y > 216 / 24389 ? 116 * Math.cbrt(y) - 16 : (24389 / 27) * y;
}

function median(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

/** L* of every pixel in a rectangle, clipped to the picture. */
function sample(png, x0, y0, x1, y1) {
  const out = [];
  for (let y = Math.max(0, Math.floor(y0)); y < Math.min(png.height, Math.ceil(y1)); y += 1) {
    for (let x = Math.max(0, Math.floor(x0)); x < Math.min(png.width, Math.ceil(x1)); x += 1) {
      const i = (y * png.width + x) * 4;
      out.push(lightness(png.data[i], png.data[i + 1], png.data[i + 2]));
    }
  }
  return out;
}

/**
 * Compares each overlay with its surroundings along the middle half of every side that is not at the window's
 * edge: inside = 8–14 px in from the boundary (the surface, past any padding hairline), edge = the 3 px straddling
 * the boundary, outside = 8–14 px out (past the hairline, inside the shadow's reach, which is part of the
 * separation). Corners are left out because of the rounding.
 */
function layering(png, box) {
  const sides = [];
  const { x, y, width: w, height: h } = box;
  const midX0 = x + w / 4, midX1 = x + (3 * w) / 4, midY0 = y + h / 4, midY1 = y + (3 * h) / 4;
  if (y > 16) sides.push({ inside: [midX0, y + 8, midX1, y + 14], edge: [midX0, y - 1, midX1, y + 2], outside: [midX0, y - 14, midX1, y - 8] });
  if (y + h < png.height - 16) sides.push({ inside: [midX0, y + h - 14, midX1, y + h - 8], edge: [midX0, y + h - 2, midX1, y + h + 1], outside: [midX0, y + h + 8, midX1, y + h + 14] });
  if (x > 16) sides.push({ inside: [x + 8, midY0, x + 14, midY1], edge: [x - 1, midY0, x + 2, midY1], outside: [x - 14, midY0, x - 8, midY1] });
  if (x + w < png.width - 16) sides.push({ inside: [x + w - 14, midY0, x + w - 8, midY1], edge: [x + w - 2, midY0, x + w + 1, midY1], outside: [x + w + 8, midY0, x + w + 14, midY1] });
  let best = null;
  for (const side of sides) {
    const outside = median(sample(png, ...side.outside));
    const inside = median(sample(png, ...side.inside));
    // The edge counts with its most different pixels, since a hairline is one pixel of the three.
    const edge = sample(png, ...side.edge).map((value) => Math.abs(value - outside)).sort((a, b) => b - a);
    const edgeDelta = edge.length ? edge[Math.floor(edge.length * 0.1)] : 0;
    const surfaceDelta = Math.abs(inside - outside);
    const score = { surfaceDelta, edgeDelta, ok: surfaceDelta >= SURFACE_DELTA_L || edgeDelta >= EDGE_DELTA_L };
    // One side that stands apart is not enough: the overlay has to be seen as a whole, so the weakest side counts.
    if (!best || (!score.ok && best.ok) || (score.ok === best.ok && score.surfaceDelta + score.edgeDelta < best.surfaceDelta + best.edgeDelta)) best = score;
  }
  return best;
}

async function audit(page, screenshotPath) {
  const findings = [];
  const measures = [];

  // Text cut off without an ellipsis.
  const clipped = await page.evaluate(() => {
    const out = [];
    const describe = (el) => {
      const id = el.id ? `#${el.id}` : "";
      const cls = typeof el.className === "string" && el.className.trim() ? `.${el.className.trim().split(/\s+/).join(".")}` : "";
      return `${el.tagName.toLowerCase()}${id}${cls} "${(el.textContent || "").trim().slice(0, 40)}"`;
    };
    const scrolls = (el) => {
      for (let node = el.parentElement; node; node = node.parentElement) {
        const style = getComputedStyle(node);
        if (/(auto|scroll)/.test(style.overflowX) && node.scrollWidth > node.clientWidth) return true;
      }
      return false;
    };
    for (const el of document.body.querySelectorAll("*")) {
      if (["INPUT", "TEXTAREA", "SELECT", "svg", "SCRIPT", "STYLE"].includes(el.tagName)) continue;
      if (el.closest("svg, [aria-hidden='true'], pre, code, textarea")) continue;
      const ownText = [...el.childNodes].some((node) => node.nodeType === Node.TEXT_NODE && node.textContent.trim());
      if (!ownText || !el.checkVisibility({ opacityProperty: true, visibilityProperty: true })) continue;
      const rect = el.getBoundingClientRect();
      if (rect.width <= 1 || rect.height <= 1) continue;
      const style = getComputedStyle(el);
      const hidesX = /(hidden|clip)/.test(style.overflowX);
      const hidesY = /(hidden|clip)/.test(style.overflowY);
      if (hidesX && el.scrollWidth > el.clientWidth + 1 && style.textOverflow !== "ellipsis") out.push(`clip: text cut off at the side, no ellipsis: ${describe(el)}`);
      else if (hidesY && el.scrollHeight > el.clientHeight + 2 && style.webkitLineClamp === "none") out.push(`clip: text cut off at the bottom: ${describe(el)}`);
      else if ((rect.right > window.innerWidth + 1 || rect.left < -1) && !scrolls(el)) out.push(`clip: text outside the window: ${describe(el)}`);
    }
    return out;
  });
  findings.push(...clipped);

  // Overlays against what is under them, on the picture just taken.
  const boxes = await page.evaluate((selector) => [...document.querySelectorAll(selector)]
    .filter((el) => el.checkVisibility({ opacityProperty: true, visibilityProperty: true }))
    // A list or menu inside a pop-up is measured as the pop-up.
    .filter((el) => !el.parentElement?.closest('.popover, [role="dialog"], [role="alertdialog"]'))
    .map((el) => {
      // A dialog's role often sits on an inner element inside the visible sheet (react-aria's Modal > Dialog): measure
      // the outermost box under the backdrop, the one that draws the surface, not the inner element on that surface.
      let surface = el;
      if (/dialog/.test(el.getAttribute("role") || "")) {
        for (let node = el.parentElement; node && node !== document.body; node = node.parentElement) {
          const box = node.getBoundingClientRect();
          if (box.width === 0 || box.height === 0 || (box.width >= window.innerWidth - 1 && box.height >= window.innerHeight - 1)) break;
          surface = node;
        }
      }
      const rect = surface.getBoundingClientRect();
      return { name: `${el.getAttribute("role") || el.className}`, x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })
    .filter((box) => box.width > 40 && box.height > 24), OVERLAYS);
  if (boxes.length > 0) {
    const png = PNG.sync.read(fs.readFileSync(screenshotPath));
    for (const box of boxes) {
      const score = layering(png, box);
      if (!score) continue;
      measures.push({ overlay: box.name, surfaceDeltaL: Number(score.surfaceDelta.toFixed(1)), edgeDeltaL: Number(score.edgeDelta.toFixed(1)) });
      if (!score.ok) {
        findings.push(`layer: ${box.name} does not stand apart from what is under it (surface ΔL* ${score.surfaceDelta.toFixed(1)} < ${SURFACE_DELTA_L}, edge ΔL* ${score.edgeDelta.toFixed(1)} < ${EDGE_DELTA_L})`);
      }
    }
  }

  // axe needs timers, and the page clock is paused for the picture; the picture is taken, so let it run.
  await page.clock.resume();
  await page.addScriptTag({ path: AXE });
  const result = await page.evaluate(async (tags) => window.axe.run(document, { runOnly: { type: "tag", values: tags }, resultTypes: ["violations"] }), AXE_TAGS);
  for (const violation of result.violations) {
    if (violation.impact !== "serious" && violation.impact !== "critical") continue;
    for (const node of violation.nodes) {
      const why = (node.any[0] || node.all[0] || node.none[0])?.message ?? violation.help;
      findings.push(`axe ${violation.id} (${violation.impact}): ${node.target.join(" ")}: ${why}`);
    }
  }
  return { findings, measures };
}

module.exports = { audit, lightness, layering };
