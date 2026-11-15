// A JSON document as the inspector shows it. The parser is our own rather than JSON.parse for three reasons a
// debugger cares about: numbers keep the digits the server sent (JSON.parse rounds 64-bit ids), keys keep their
// order, and a key that appears twice shows twice.

export type JsonNode =
  | { kind: "object"; entries: Array<[string, JsonNode]> }
  | { kind: "array"; items: JsonNode[] }
  | { kind: "string"; value: string }
  | { kind: "number"; raw: string }
  | { kind: "boolean"; value: boolean }
  | { kind: "null" };

/** Deeper nesting than this is not shown as a tree. */
const MAX_DEPTH = 200;

class Reader {
  at = 0;
  constructor(readonly text: string) {}

  space() {
    while (this.at < this.text.length) {
      const code = this.text.charCodeAt(this.at);
      if (code !== 0x20 && code !== 0x0a && code !== 0x0d && code !== 0x09) break;
      this.at += 1;
    }
  }

  value(depth: number): JsonNode {
    if (depth > MAX_DEPTH) throw new Error("too deep");
    this.space();
    const start = this.text[this.at];
    if (start === "{") return this.object(depth);
    if (start === "[") return this.array(depth);
    if (start === '"') return { kind: "string", value: this.string() };
    if (this.word("true")) return { kind: "boolean", value: true };
    if (this.word("false")) return { kind: "boolean", value: false };
    if (this.word("null")) return { kind: "null" };
    return this.number();
  }

  word(word: string): boolean {
    if (!this.text.startsWith(word, this.at)) return false;
    this.at += word.length;
    return true;
  }

  number(): JsonNode {
    const match = /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?/.exec(this.text.slice(this.at, this.at + 400));
    if (!match) throw new Error("unexpected character");
    this.at += match[0].length;
    return { kind: "number", raw: match[0] };
  }

  string(): string {
    this.at += 1;
    let result = "";
    for (;;) {
      if (this.at >= this.text.length) throw new Error("unterminated string");
      const char = this.text[this.at];
      const code = char.charCodeAt(0);
      if (char === '"') {
        this.at += 1;
        return result;
      }
      if (code < 0x20) throw new Error("control character in string");
      if (char !== "\\") {
        result += char;
        this.at += 1;
        continue;
      }
      const escape = this.text[this.at + 1];
      this.at += 2;
      switch (escape) {
        case '"': result += '"'; break;
        case "\\": result += "\\"; break;
        case "/": result += "/"; break;
        case "b": result += "\b"; break;
        case "f": result += "\f"; break;
        case "n": result += "\n"; break;
        case "r": result += "\r"; break;
        case "t": result += "\t"; break;
        case "u": {
          const hex = this.text.slice(this.at, this.at + 4);
          if (!/^[0-9a-fA-F]{4}$/.test(hex)) throw new Error("bad unicode escape");
          result += String.fromCharCode(parseInt(hex, 16));
          this.at += 4;
          break;
        }
        default:
          throw new Error("bad escape");
      }
    }
  }

  object(depth: number): JsonNode {
    this.at += 1;
    const entries: Array<[string, JsonNode]> = [];
    this.space();
    if (this.text[this.at] === "}") {
      this.at += 1;
      return { kind: "object", entries };
    }
    for (;;) {
      this.space();
      if (this.text[this.at] !== '"') throw new Error("expected a key");
      const key = this.string();
      this.space();
      if (this.text[this.at] !== ":") throw new Error("expected a colon");
      this.at += 1;
      entries.push([key, this.value(depth + 1)]);
      this.space();
      const next = this.text[this.at];
      this.at += 1;
      if (next === "}") return { kind: "object", entries };
      if (next !== ",") throw new Error("expected a comma");
    }
  }

  array(depth: number): JsonNode {
    this.at += 1;
    const items: JsonNode[] = [];
    this.space();
    if (this.text[this.at] === "]") {
      this.at += 1;
      return { kind: "array", items };
    }
    for (;;) {
      items.push(this.value(depth + 1));
      this.space();
      const next = this.text[this.at];
      this.at += 1;
      if (next === "]") return { kind: "array", items };
      if (next !== ",") throw new Error("expected a comma");
    }
  }
}

/** The document, or null when the text is not one complete JSON value (a cut body, JSON lines, plain text). */
export function parseJson(text: string): JsonNode | null {
  const reader = new Reader(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
  try {
    const node = reader.value(0);
    reader.space();
    return reader.at === reader.text.length ? node : null;
  } catch {
    return null;
  }
}

function isContainer(node: JsonNode): node is Extract<JsonNode, { kind: "object" | "array" }> {
  return node.kind === "object" || node.kind === "array";
}

/** How many children an object or an array has; 0 for a scalar. */
export function childCount(node: JsonNode): number {
  return node.kind === "object" ? node.entries.length : node.kind === "array" ? node.items.length : 0;
}

/** One line of the tree. `id` is stable for the document (positions, so a repeated key still gets its own). */
export interface JsonRow {
  id: string;
  /** The id of the row this one sits under; null for the root. */
  parent: string | null;
  depth: number;
  /** The object key; absent for array items and the root. */
  name?: string;
  /** The position in the parent array; absent for object members and the root. */
  index?: number;
  node: JsonNode;
  /** JSONPath to this value, for copying: `$.items[0].title`. */
  path: string;
  /** For an object or an array with children: whether its children are listed. */
  open?: boolean;
}

function memberPath(parent: string, name: string): string {
  return /^[A-Za-z_$][\w$]*$/.test(name) ? `${parent}.${name}` : `${parent}[${JSON.stringify(name)}]`;
}

/** The visible lines of the tree, top to bottom, given which containers are folded. */
export function jsonRows(root: JsonNode, collapsed: ReadonlySet<string>): JsonRow[] {
  const rows: JsonRow[] = [];
  const visit = (node: JsonNode, row: Omit<JsonRow, "node" | "open">) => {
    const open = isContainer(node) && childCount(node) > 0 ? !collapsed.has(row.id) : undefined;
    rows.push({ ...row, node, open });
    if (!open) return;
    if (node.kind === "object") {
      node.entries.forEach(([name, child], position) => visit(child, {
        id: `${row.id}/${position}`, parent: row.id, depth: row.depth + 1, name, path: memberPath(row.path, name),
      }));
    } else if (node.kind === "array") {
      node.items.forEach((child, position) => visit(child, {
        id: `${row.id}/${position}`, parent: row.id, depth: row.depth + 1, index: position, path: `${row.path}[${position}]`,
      }));
    }
  };
  visit(root, { id: "$", parent: null, depth: 0, path: "$" });
  return rows;
}

/** Every container's id, for "fold all". The root stays open, or the tree would be one line. */
export function containerIds(root: JsonNode, fromDepth = 1): string[] {
  const ids: string[] = [];
  const visit = (node: JsonNode, id: string, depth: number) => {
    if (!isContainer(node) || childCount(node) === 0) return;
    if (depth >= fromDepth) ids.push(id);
    const children = node.kind === "object" ? node.entries.map(([, child]) => child) : node.items;
    children.forEach((child, position) => visit(child, `${id}/${position}`, depth + 1));
  };
  visit(root, "$", 0);
  return ids;
}

/** How many values the document holds, counting containers. Stops counting at `limit`. */
export function nodeCount(root: JsonNode, limit = Infinity): number {
  let count = 0;
  const visit = (node: JsonNode) => {
    if (count >= limit) return;
    count += 1;
    if (node.kind === "object") for (const [, child] of node.entries) visit(child);
    else if (node.kind === "array") for (const child of node.items) visit(child);
  };
  visit(root);
  return count;
}

/** A small document opens fully; a large one opens two levels, so the first screen is an outline. */
export function initiallyCollapsed(root: JsonNode): Set<string> {
  return nodeCount(root, 401) > 400 ? new Set(containerIds(root, 2)) : new Set();
}

export type JsonTokenKind = "punctuation" | "key" | "string" | "number" | "literal" | "space";
export interface JsonToken {
  kind: JsonTokenKind;
  text: string;
}

/** The document indented by two spaces, as tokens to colour. Numbers are written as received. */
export function prettyTokens(root: JsonNode): JsonToken[] {
  const tokens: JsonToken[] = [];
  const push = (kind: JsonTokenKind, text: string) => tokens.push({ kind, text });
  const visit = (node: JsonNode, indent: string) => {
    switch (node.kind) {
      case "string": push("string", JSON.stringify(node.value)); return;
      case "number": push("number", node.raw); return;
      case "boolean": push("literal", String(node.value)); return;
      case "null": push("literal", "null"); return;
      case "object":
      case "array": {
        const [open, close] = node.kind === "object" ? ["{", "}"] : ["[", "]"];
        const count = childCount(node);
        if (count === 0) {
          push("punctuation", open + close);
          return;
        }
        const inner = `${indent}  `;
        push("punctuation", open);
        for (let position = 0; position < count; position += 1) {
          push("space", `\n${inner}`);
          if (node.kind === "object") {
            const [name, child] = node.entries[position];
            push("key", JSON.stringify(name));
            push("punctuation", ": ");
            visit(child, inner);
          } else {
            visit(node.items[position], inner);
          }
          if (position < count - 1) push("punctuation", ",");
        }
        push("space", `\n${indent}`);
        push("punctuation", close);
      }
    }
  };
  visit(root, "");
  return tokens;
}

/** The value as indented JSON text, for the clipboard. */
export function prettyJson(node: JsonNode): string {
  return prettyTokens(node).map((token) => token.text).join("");
}

/** What copying one tree row puts on the clipboard: a string's own text, otherwise the value as JSON. */
export function copyText(node: JsonNode): string {
  return node.kind === "string" ? node.value : prettyJson(node);
}
