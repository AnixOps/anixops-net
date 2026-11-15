// Which screenshot changes a pull request has acknowledged, read from its description.
//
// The author lists the shots they expect to change on a line that starts with "界面变化：" (or "UI changes:"),
// separated by commas or spaces; `*` stands for any run of characters, so `debug-inspector-*` covers a family.
// Several such lines add up. Nothing else in the description counts, so a change is never acknowledged by accident.

const LINE = /^[ \t]*(?:[-*][ \t]+)?(?:界面变化|UI changes)[ \t]*[:：][ \t]*(.*)$/gim;

/** The acknowledged names and patterns, in the order written, without duplicates. */
function acknowledged(description) {
  const names = [];
  for (const match of String(description ?? "").matchAll(LINE)) {
    for (const item of match[1].split(/[\s,，、;；]+/)) {
      const name = item.replace(/^`|`$/g, "").replace(/\.png$/i, "");
      if (/^[A-Za-z0-9*][A-Za-z0-9*._-]*$/.test(name) && !names.includes(name)) names.push(name);
    }
  }
  return names;
}

function matches(pattern, name) {
  const expression = pattern.split("*").map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(".*");
  return new RegExp(`^${expression}$`).test(name);
}

/**
 * Splits the changed shots into the ones the description covers and the ones it does not, and lists the
 * acknowledgements that cover nothing (a stale line, or a typo).
 */
function review(changed, description) {
  const patterns = acknowledged(description);
  const covered = changed.filter((name) => patterns.some((pattern) => matches(pattern, name)));
  const uncovered = changed.filter((name) => !covered.includes(name));
  const unused = patterns.filter((pattern) => !changed.some((name) => matches(pattern, name)));
  return { covered, uncovered, unused };
}

module.exports = { acknowledged, matches, review };
