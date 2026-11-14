import { JsonNode, parseJson } from "./json-view";

// What a captured body is and the ways the inspector can show it. Kept apart from the components so the choices are
// tested without a window.

export type BodyKind = "json" | "image" | "form" | "text" | "binary";
/** tree: folding JSON tree. pretty: indented and coloured. table: name/value pairs. raw: the text as received. */
export type BodyMode = "tree" | "pretty" | "preview" | "table" | "raw" | "hex";

/** "Application/JSON; charset=utf-8" → "application/json". */
export function mediaType(contentType: string | null | undefined): string {
  return (contentType ?? "").split(";")[0].trim().toLowerCase();
}

/** The image types a webview draws from a data: URL. SVG is drawn as an image, so nothing in it runs. */
const imageTypes = new Set([
  "image/png", "image/jpeg", "image/gif", "image/webp", "image/avif", "image/bmp", "image/svg+xml", "image/x-icon", "image/vnd.microsoft.icon",
]);

export function imageType(contentType: string | null | undefined): string | null {
  const media = mediaType(contentType);
  return imageTypes.has(media) ? media : null;
}

export interface BodyShape {
  kind: BodyKind;
  /** The parsed document when `kind` is "json". */
  json?: JsonNode;
}

/**
 * What the body is, from its Content-Type and its text (`null` when the bytes are not text). JSON is recognised by
 * its type or, when a server labels it text/plain or not at all, by parsing; a cut JSON body does not parse and is
 * shown as text.
 */
export function bodyShape(contentType: string | null | undefined, text: string | null): BodyShape {
  if (imageType(contentType)) return { kind: "image" };
  if (text === null) return { kind: "binary" };
  const media = mediaType(contentType);
  if (media === "application/x-www-form-urlencoded") return { kind: "form" };
  const start = text.trimStart()[0];
  const maybeJson = media.endsWith("/json") || media.endsWith("+json") || ((start === "{" || start === "[") && !/(html|xml|javascript|css)/.test(media));
  if (maybeJson) {
    const json = parseJson(text);
    if (json) return { kind: "json", json };
  }
  return { kind: "text" };
}

/** The ways to show a body of this kind, the default first. */
export function bodyModes(kind: BodyKind): BodyMode[] {
  switch (kind) {
    case "json": return ["tree", "pretty", "raw", "hex"];
    case "image": return ["preview", "hex"];
    case "form": return ["table", "raw", "hex"];
    case "text": return ["raw", "hex"];
    case "binary": return ["hex"];
  }
}

/** The mode to show: the one the user chose last for this kind when it still applies, else the kind's default. */
export function chosenMode(kind: BodyKind, preferred: BodyMode | undefined): BodyMode {
  const modes = bodyModes(kind);
  return preferred && modes.includes(preferred) ? preferred : modes[0];
}

function decodePart(part: string): string {
  const spaced = part.replace(/\+/g, " ");
  try {
    return decodeURIComponent(spaced);
  } catch {
    // Not valid percent-encoding: show it as written.
    return spaced;
  }
}

/** `a=1&b=x%20y` as name/value pairs, decoded; a name without `=` has an empty value. */
export function formPairs(text: string): Array<[string, string]> {
  return text.split("&").filter((part) => part.length > 0).map((part): [string, string] => {
    const equals = part.indexOf("=");
    return equals < 0 ? [decodePart(part), ""] : [decodePart(part.slice(0, equals)), decodePart(part.slice(equals + 1))];
  });
}

/** The query parameters of a URL, in order, decoded. */
export function queryPairs(url: string): Array<[string, string]> {
  const hash = url.indexOf("#");
  const address = hash < 0 ? url : url.slice(0, hash);
  const question = address.indexOf("?");
  return question < 0 ? [] : formPairs(address.slice(question + 1));
}

export interface WaterfallBar {
  /** Where the bar starts and how wide it is, as fractions of the track. */
  left: number;
  width: number;
  /** Milliseconds from start to end; null while the response is still arriving. */
  took: number | null;
}

/**
 * The requests of one connection on a shared time axis, from the first start to the last end. A request with no
 * end yet runs to `now`. Bars keep a minimum width so an instant request is still visible.
 */
export function waterfall(exchanges: Array<{ started_at_ms: number; ended_at_ms?: number | null }>, now: number): WaterfallBar[] {
  if (exchanges.length === 0) return [];
  const from = Math.min(...exchanges.map((exchange) => exchange.started_at_ms));
  const to = Math.max(...exchanges.map((exchange) => exchange.ended_at_ms ?? Math.max(now, exchange.started_at_ms)));
  const span = Math.max(1, to - from);
  const least = 0.01;
  return exchanges.map((exchange) => {
    const end = exchange.ended_at_ms ?? Math.max(now, exchange.started_at_ms);
    const width = Math.max(least, (end - exchange.started_at_ms) / span);
    const left = Math.min((exchange.started_at_ms - from) / span, 1 - width);
    return { left, width, took: exchange.ended_at_ms == null ? null : Math.max(0, exchange.ended_at_ms - exchange.started_at_ms) };
  });
}
