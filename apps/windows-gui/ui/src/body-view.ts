import { decodeProtobuf, grpcFrames, GrpcFrame, isGrpcType, isProtobufType, looksLikeProtobuf, ProtoMessage, sniffImage } from "./binary-view";
import { JsonNode, parseJson } from "./json-view";

// What a captured body is and the ways the inspector can show it. Kept apart from the components so the choices are
// tested without a window.

export type BodyKind = "json" | "image" | "form" | "protobuf" | "text" | "binary";
/**
 * tree: folding JSON tree. pretty: indented and coloured. table: name/value pairs. protobuf: fields decoded without
 * a schema. raw: the text as received.
 */
export type BodyMode = "tree" | "pretty" | "preview" | "table" | "protobuf" | "raw" | "hex";

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

/**
 * Types another app may be asked to open (`desktop.openBody`), the same list as `body_type` in
 * crates/desktop-mitm/src/capture.rs. Types that run code where they open (HTML, SVG, scripts, XML, CSV) and
 * unknown bytes are only saved.
 */
const openable = new Set([
  "image/png", "image/jpeg", "image/jpg", "image/gif", "image/webp", "image/avif", "image/bmp", "image/x-icon", "image/vnd.microsoft.icon",
  "application/pdf", "text/plain", "video/mp4", "video/webm", "audio/mpeg", "audio/mp4", "audio/ogg", "audio/wav", "audio/x-wav", "application/json",
]);

export function opensElsewhere(contentType: string | null | undefined): boolean {
  const media = mediaType(contentType);
  return openable.has(media) || media.endsWith("+json");
}

export interface BodyShape {
  kind: BodyKind;
  /** The parsed document when `kind` is "json". */
  json?: JsonNode;
  /** The picture's type when `kind` is "image": from Content-Type, or from the first bytes when the type is generic. */
  image?: string;
  /** The decoded message when `kind` is "protobuf", or when a "binary" body also reads as one. */
  proto?: ProtoMessage;
  /** The frames of a gRPC body (`kind` "protobuf"), each decoded on its own. */
  grpc?: { frames: GrpcFrame[]; cut: boolean };
}

/**
 * What the body is, from its Content-Type, its text (`null` when the bytes are not text) and its bytes. JSON is
 * recognised by its type or, when a server labels it text/plain or not at all, by parsing; a cut JSON body does not
 * parse and is shown as text. A picture or Protobuf sent with a generic type is recognised by its bytes; Protobuf
 * then opens as hex with the decoded view one click away, since random bytes can read as a message too. `cut` is
 * whether capture kept only the start of the body.
 */
export function bodyShape(contentType: string | null | undefined, text: string | null, bytes?: Uint8Array, cut = false): BodyShape {
  const media = mediaType(contentType);
  const labelled = imageType(contentType);
  if (labelled) return { kind: "image", image: labelled };
  if (bytes && isGrpcType(media)) {
    const grpc = grpcFrames(bytes);
    if (grpc) return { kind: "protobuf", grpc };
  }
  if (bytes && isProtobufType(media)) {
    const proto = decodeProtobuf(bytes, cut);
    if (proto) return { kind: "protobuf", proto };
  }
  if (text === null) {
    const sniffed = bytes ? sniffImage(bytes) : null;
    if (sniffed && !media.startsWith("text/")) return { kind: "image", image: sniffed };
    const proto = bytes ? looksLikeProtobuf(bytes) : null;
    return proto ? { kind: "binary", proto } : { kind: "binary" };
  }
  if (media === "application/x-www-form-urlencoded") return { kind: "form" };
  const start = text.trimStart()[0];
  const maybeJson = media.endsWith("/json") || media.endsWith("+json") || ((start === "{" || start === "[") && !/(html|xml|javascript|css)/.test(media));
  if (maybeJson) {
    const json = parseJson(text);
    if (json) return { kind: "json", json };
  }
  return { kind: "text" };
}

/** The ways to show a body of this kind, the default first. `proto`: a binary body also reads as Protobuf. */
export function bodyModes(kind: BodyKind, proto = false): BodyMode[] {
  switch (kind) {
    case "json": return ["tree", "pretty", "raw", "hex"];
    case "image": return ["preview", "hex"];
    case "form": return ["table", "raw", "hex"];
    case "protobuf": return ["protobuf", "hex"];
    case "text": return ["raw", "hex"];
    case "binary": return proto ? ["hex", "protobuf"] : ["hex"];
  }
}

/** The mode to show: the one the user chose last for this kind when it still applies, else the kind's default. */
export function chosenMode(kind: BodyKind, preferred: BodyMode | undefined, proto = false): BodyMode {
  const modes = bodyModes(kind, proto);
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
