// Binary bodies the inspector can still show as something: pictures recognised by their first bytes, and Protobuf
// (bare, or framed as gRPC) decoded without a schema, the way `protoc --decode_raw` does. Every reader here works
// on bytes from the network, so each one checks every length against what is there, stops at fixed limits and
// answers null instead of throwing. The same rules and limits are kept on Apple (`CaptureBinary.swift`) and Android
// (`CaptureBinary.kt`); docs/body-preview.md describes them.

/** Pictures larger than this many pixels are not drawn: a few kilobytes of PNG can claim 65535 × 65535. */
export const MAX_IMAGE_PIXELS = 67_108_864;

/** The most bytes of a body decoded as Protobuf; the rest is left out, as with a body capture cut. */
export const PROTO_MAX_BYTES = 2 * 1024 * 1024;
/** How deep messages inside messages are opened; deeper ones are shown as bytes. */
export const PROTO_MAX_DEPTH = 16;
/** The most fields shown in all; decoding stops there and says so. */
export const PROTO_MAX_FIELDS = 20_000;
/**
 * How many bytes, in all, are looked at again to tell whether a length-delimited field is text or a message. Past
 * it, fields are shown as bytes; it keeps deep nesting from costing a pass over the body per level.
 */
export const PROTO_MAX_SCAN = 16 * 1024 * 1024;
/** The most gRPC messages read from one body. */
export const GRPC_MAX_FRAMES = 1_000;
/** A string longer than this is cut on screen. */
const STRING_SHOWN = 1_000;
/** Bytes that are neither text nor a message show this many bytes in hex. */
const BYTES_SHOWN = 32;

/** The picture type of `bytes` from its signature, or null. Only types a webview draws are named. */
export function sniffImage(bytes: Uint8Array): string | null {
  const at = (offset: number, text: string) => text.split("").every((char, index) => bytes[offset + index] === char.charCodeAt(0));
  if (bytes.length >= 8 && bytes[0] === 0x89 && at(1, "PNG\r\n\u001a\n")) return "image/png";
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return "image/jpeg";
  if (bytes.length >= 6 && (at(0, "GIF87a") || at(0, "GIF89a"))) return "image/gif";
  if (bytes.length >= 12 && at(0, "RIFF") && at(8, "WEBP")) return "image/webp";
  if (bytes.length >= 12 && at(4, "ftyp") && (at(8, "avif") || at(8, "avis"))) return "image/avif";
  // "BM" alone is too common in other data: the reserved words must be zero and the header size a known one.
  if (bytes.length >= 26 && at(0, "BM") && bytes[6] === 0 && bytes[7] === 0 && bytes[8] === 0 && bytes[9] === 0) {
    const header = u32le(bytes, 14);
    if (header === 12 || header === 40 || header === 52 || header === 56 || header === 108 || header === 124) return "image/bmp";
  }
  return null;
}

function u16be(bytes: Uint8Array, offset: number): number {
  return (bytes[offset] << 8) | bytes[offset + 1];
}

function u16le(bytes: Uint8Array, offset: number): number {
  return bytes[offset] | (bytes[offset + 1] << 8);
}

function u24le(bytes: Uint8Array, offset: number): number {
  return bytes[offset] | (bytes[offset + 1] << 8) | (bytes[offset + 2] << 16);
}

function u32be(bytes: Uint8Array, offset: number): number {
  return ((bytes[offset] << 24) >>> 0) + ((bytes[offset + 1] << 16) | (bytes[offset + 2] << 8) | bytes[offset + 3]);
}

function u32le(bytes: Uint8Array, offset: number): number {
  return ((bytes[offset + 3] << 24) >>> 0) + ((bytes[offset + 2] << 16) | (bytes[offset + 1] << 8) | bytes[offset]);
}

/**
 * The width and height a picture says it has, read from its header without decoding it; null when the header is
 * not there or the type is not one read here (the picture is then drawn and fails on its own if it is damaged).
 */
export function imageDimensions(bytes: Uint8Array, type: string): { width: number; height: number } | null {
  const has = (end: number) => bytes.length >= end;
  switch (type) {
    case "image/png":
      return has(24) && String.fromCharCode(...bytes.subarray(12, 16)) === "IHDR" ? { width: u32be(bytes, 16), height: u32be(bytes, 20) } : null;
    case "image/gif":
      return has(10) ? { width: u16le(bytes, 6), height: u16le(bytes, 8) } : null;
    case "image/bmp": {
      if (!has(26)) return null;
      if (u32le(bytes, 14) === 12) return { width: u16le(bytes, 18), height: u16le(bytes, 20) };
      const height = u32le(bytes, 22) | 0;
      return { width: Math.abs(u32le(bytes, 18) | 0), height: Math.abs(height) };
    }
    case "image/webp": {
      if (!has(30)) return null;
      const chunk = String.fromCharCode(...bytes.subarray(12, 16));
      if (chunk === "VP8X") return { width: 1 + u24le(bytes, 24), height: 1 + u24le(bytes, 27) };
      if (chunk === "VP8L") {
        const b1 = bytes[22], b2 = bytes[23], b3 = bytes[24];
        return { width: 1 + (bytes[21] | ((b1 & 0x3f) << 8)), height: 1 + ((b1 >> 6) | (b2 << 2) | ((b3 & 0x0f) << 10)) };
      }
      if (chunk === "VP8 ") return { width: u16le(bytes, 26) & 0x3fff, height: u16le(bytes, 28) & 0x3fff };
      return null;
    }
    case "image/jpeg":
      return jpegDimensions(bytes);
    case "image/avif":
      return avifDimensions(bytes);
    default:
      return null;
  }
}

/** The size in the first start-of-frame segment, walking the segments by their lengths. */
function jpegDimensions(bytes: Uint8Array): { width: number; height: number } | null {
  let offset = 2;
  while (offset + 4 <= bytes.length) {
    if (bytes[offset] !== 0xff) return null;
    const marker = bytes[offset + 1];
    if (marker === 0xff) {
      offset += 1;
      continue;
    }
    if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
      offset += 2;
      continue;
    }
    const length = u16be(bytes, offset + 2);
    if (length < 2) return null;
    const startOfFrame = marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc;
    if (startOfFrame) {
      return offset + 9 <= bytes.length ? { width: u16be(bytes, offset + 7), height: u16be(bytes, offset + 5) } : null;
    }
    offset += 2 + length;
  }
  return null;
}

/** The size in the first `ispe` (image spatial extent) property near the start of the file. */
function avifDimensions(bytes: Uint8Array): { width: number; height: number } | null {
  const end = Math.min(bytes.length, 64 * 1024) - 16;
  for (let offset = 4; offset <= end; offset += 1) {
    if (bytes[offset] === 0x69 && bytes[offset + 1] === 0x73 && bytes[offset + 2] === 0x70 && bytes[offset + 3] === 0x65) {
      return { width: u32be(bytes, offset + 8), height: u32be(bytes, offset + 12) };
    }
  }
  return null;
}

/** Whether a picture of this size is small enough to draw. Unknown sizes are drawn. */
export function drawable(size: { width: number; height: number } | null): boolean {
  return size === null || size.width * size.height <= MAX_IMAGE_PIXELS;
}

// Protobuf ----------------------------------------------------------------------------------------------------------

export type ProtoValue =
  | { wire: "varint"; value: bigint }
  | { wire: "fixed64"; value: bigint }
  | { wire: "fixed32"; value: number }
  /** Length-delimited: a message when the bytes decode as one, else text when they are text, else bytes. */
  | { wire: "bytes"; size: number; message?: ProtoField[]; text?: string; head?: Uint8Array };

export interface ProtoField {
  number: number;
  value: ProtoValue;
}

export interface ProtoMessage {
  fields: ProtoField[];
  /** Not every byte was decoded: the body was cut, too long, or had more fields than are shown. */
  cut: boolean;
}

/** Media types that carry one bare Protobuf message. */
export function isProtobufType(media: string): boolean {
  if (isGrpcType(media)) return false;
  return media === "application/x-protobuf" || media === "application/protobuf" || media === "application/x-google-protobuf"
    || media === "application/vnd.google.protobuf" || media === "application/x-protobuffer" || media.endsWith("+proto");
}

/** gRPC and gRPC-Web in their binary form, where each message is framed. `grpc-web-text` is base64 text. */
export function isGrpcType(media: string): boolean {
  return media === "application/grpc" || media === "application/grpc+proto" || media === "application/grpc-web" || media === "application/grpc-web+proto";
}

const MAX_FIELD_NUMBER = 536_870_911;

class Reader {
  constructor(readonly bytes: Uint8Array, public offset: number, readonly end: number) {}

  /** A tag or a length: at most five bytes, as a number. */
  small(): number | null {
    let result = 0;
    for (let index = 0; index < 5; index += 1) {
      if (this.offset >= this.end) return null;
      const byte = this.bytes[this.offset++];
      result += (byte & 0x7f) * 2 ** (7 * index);
      if (byte < 0x80) return result;
    }
    return null;
  }

  /** A value: up to ten bytes, 64 bits. */
  varint(): bigint | null {
    let result = 0n;
    for (let shift = 0n; shift < 70n; shift += 7n) {
      if (this.offset >= this.end) return null;
      const byte = this.bytes[this.offset++];
      // The tenth byte may only carry the top bit of a 64-bit value.
      if (shift === 63n && byte > 1) return null;
      result |= BigInt(byte & 0x7f) << shift;
      if (byte < 0x80) return result;
    }
    return null;
  }
}

interface Budget {
  fields: number;
  /** Bytes left for telling text and messages apart ({@link PROTO_MAX_SCAN}). */
  scan: number;
  /** Set once the field limit stopped decoding. */
  spent: boolean;
}

/**
 * The fields of one message in `bytes[start, end)`; null when the bytes are not a whole message. With `partial`,
 * fields up to the first one that does not decode are kept (the body was cut there).
 */
function readMessage(bytes: Uint8Array, start: number, end: number, depth: number, budget: Budget, partial: boolean): ProtoMessage | null {
  const reader = new Reader(bytes, start, end);
  const fields: ProtoField[] = [];
  const stop = (): ProtoMessage | null => (partial && fields.length > 0 ? { fields, cut: true } : null);
  while (reader.offset < end) {
    if (budget.fields <= 0) {
      budget.spent = true;
      return { fields, cut: true };
    }
    const tag = reader.small();
    if (tag === null) return stop();
    const number = Math.floor(tag / 8);
    if (number < 1 || number > MAX_FIELD_NUMBER) return stop();
    let value: ProtoValue;
    switch (tag % 8) {
      case 0: {
        const read = reader.varint();
        if (read === null) return stop();
        value = { wire: "varint", value: read };
        break;
      }
      case 1: {
        if (end - reader.offset < 8) return stop();
        const low = BigInt(u32le(bytes, reader.offset));
        const high = BigInt(u32le(bytes, reader.offset + 4));
        value = { wire: "fixed64", value: (high << 32n) | low };
        reader.offset += 8;
        break;
      }
      case 5: {
        if (end - reader.offset < 4) return stop();
        value = { wire: "fixed32", value: u32le(bytes, reader.offset) };
        reader.offset += 4;
        break;
      }
      case 2: {
        const length = reader.small();
        if (length === null || length > end - reader.offset) return stop();
        const from = reader.offset;
        const to = from + length;
        value = lengthDelimited(bytes, from, to, depth, budget);
        reader.offset = to;
        break;
      }
      default:
        // Groups (3, 4) are long deprecated and 6, 7 do not exist: not a message this reader shows.
        return stop();
    }
    budget.fields -= 1;
    fields.push({ number, value });
  }
  return { fields, cut: false };
}

function lengthDelimited(bytes: Uint8Array, from: number, to: number, depth: number, budget: Budget): ProtoValue {
  const slice = bytes.subarray(from, to);
  const size = to - from;
  if (size > budget.scan) return { wire: "bytes", size, head: slice.subarray(0, BYTES_SHOWN) };
  budget.scan -= size;
  const text = printable(slice);
  if (text !== null) return { wire: "bytes", size, text };
  if (depth + 1 < PROTO_MAX_DEPTH) {
    // A failed try gives back the fields it counted.
    const before = budget.fields;
    const inner = readMessage(bytes, from, to, depth + 1, budget, false);
    // A message stopped by the field limit is kept as far as it went; the limit marks the whole body as cut.
    if (inner && (inner.fields.length > 0 || inner.cut)) return { wire: "bytes", size, message: inner.fields };
    budget.fields = before;
  }
  return { wire: "bytes", size, head: slice.subarray(0, BYTES_SHOWN) };
}

// A leading byte order mark is kept, as the Apple and Android readers keep it.
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

/** The bytes as UTF-8 text when they are text: no control characters besides tab, line feed and carriage return. */
function printable(bytes: Uint8Array): string | null {
  for (const byte of bytes) {
    if ((byte < 0x20 && byte !== 0x09 && byte !== 0x0a && byte !== 0x0d) || byte === 0x7f) return null;
  }
  try {
    return decoder.decode(bytes);
  } catch {
    return null;
  }
}

/**
 * `bytes` as one Protobuf message without a schema; null when they are not one. A body that was cut (`cut`) or is
 * longer than {@link PROTO_MAX_BYTES} keeps the fields before the cut. An empty body is an empty message.
 */
export function decodeProtobuf(bytes: Uint8Array, cut = false): ProtoMessage | null {
  const end = Math.min(bytes.length, PROTO_MAX_BYTES);
  const shortened = end < bytes.length;
  const budget: Budget = { fields: PROTO_MAX_FIELDS, scan: PROTO_MAX_SCAN, spent: false };
  const message = readMessage(bytes, 0, end, 0, budget, cut || shortened);
  if (!message) return null;
  return { fields: message.fields, cut: message.cut || shortened || budget.spent };
}

/** Whether a binary body of unknown type also reads as a Protobuf message, worth offering as a second view. */
export function looksLikeProtobuf(bytes: Uint8Array): ProtoMessage | null {
  if (bytes.length === 0) return null;
  const message = decodeProtobuf(bytes);
  return message && !message.cut && message.fields.length > 0 ? message : null;
}

export interface GrpcFrame {
  /** Compressed with the stream's `grpc-encoding`; the message is shown as bytes. */
  compressed: boolean;
  /** A gRPC-Web trailer frame: `name: value` lines. */
  trailers: boolean;
  /** The frame's length as its header gives it. */
  size: number;
  /** The bytes of the frame that are there (fewer than `size` when the body was cut). */
  data: Uint8Array;
}

/** The length-prefixed frames of a gRPC body; null when the body does not start with one. */
export function grpcFrames(bytes: Uint8Array): { frames: GrpcFrame[]; cut: boolean } | null {
  const frames: GrpcFrame[] = [];
  let offset = 0;
  while (offset < bytes.length) {
    if (frames.length >= GRPC_MAX_FRAMES) return { frames, cut: true };
    if (bytes.length - offset < 5) return frames.length > 0 ? { frames, cut: true } : null;
    const flag = bytes[offset];
    if ((flag & 0x7e) !== 0) return frames.length > 0 ? { frames, cut: true } : null;
    const size = u32be(bytes, offset + 1);
    const start = offset + 5;
    const end = Math.min(bytes.length, start + size);
    frames.push({ compressed: (flag & 0x01) !== 0, trailers: (flag & 0x80) !== 0, size, data: bytes.subarray(start, end) });
    if (start + size > bytes.length) return { frames, cut: true };
    offset = end;
  }
  return frames.length > 0 ? { frames, cut: false } : null;
}

/** The words around decoded Protobuf, in the window's language. */
export interface ProtoLabels {
  /** The heading of the `index`th (from 1) gRPC message of `size` bytes. */
  message: (index: number, size: number) => string;
  /** A compressed message that is shown as bytes. */
  compressed: string;
  /** The heading of gRPC-Web trailers. */
  trailers: string;
  /** A message that does not decode, shown as bytes. */
  undecoded: string;
  /** The end of what is shown when not everything was. */
  more: string;
}

function hexBytes(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(" ");
}

function quoted(text: string): string {
  const shown = text.length > STRING_SHOWN ? `${text.slice(0, STRING_SHOWN)}…` : text;
  return `"${shown.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n").replace(/\r/g, "\\r").replace(/\t/g, "\\t")}"`;
}

const TWO_63 = 1n << 63n;

function fieldLines(fields: ProtoField[], indent: string, lines: string[]) {
  for (const { number, value } of fields) {
    switch (value.wire) {
      case "varint":
        lines.push(`${indent}${number}: ${value.value}${value.value >= TWO_63 ? ` (${value.value - (1n << 64n)})` : ""}`);
        break;
      case "fixed64":
        lines.push(`${indent}${number}: 0x${value.value.toString(16).padStart(16, "0")} (fixed64)`);
        break;
      case "fixed32":
        lines.push(`${indent}${number}: 0x${value.value.toString(16).padStart(8, "0")} (fixed32)`);
        break;
      case "bytes":
        if (value.message) {
          lines.push(`${indent}${number} {`);
          fieldLines(value.message, `${indent}  `, lines);
          lines.push(`${indent}}`);
        } else if (value.text !== undefined) {
          lines.push(`${indent}${number}: ${quoted(value.text)}`);
        } else {
          const head = value.head ?? new Uint8Array();
          lines.push(`${indent}${number}: <${hexBytes(head)}${value.size > head.length ? " …" : ""}> (${value.size} B)`);
        }
        break;
    }
  }
}

/** A decoded message as text, one field per line, in `protoc --decode_raw` style. */
export function protoText(message: ProtoMessage, labels: Pick<ProtoLabels, "more">): string {
  const lines: string[] = [];
  fieldLines(message.fields, "", lines);
  if (message.cut) lines.push(labels.more);
  return lines.join("\n");
}

/** A gRPC body as text: each message under a heading, decoded where it can be. */
export function grpcText(body: { frames: GrpcFrame[]; cut: boolean }, labels: ProtoLabels): string {
  const parts: string[] = [];
  let index = 0;
  for (const frame of body.frames) {
    if (frame.trailers) {
      parts.push(`# ${labels.trailers}\n${printable(frame.data) ?? hexBytes(frame.data.subarray(0, BYTES_SHOWN))}`.trimEnd());
      continue;
    }
    index += 1;
    const heading = `# ${labels.message(index, frame.size)}`;
    const whole = frame.data.length === frame.size;
    if (frame.compressed) {
      parts.push(`${heading}\n${labels.compressed}`);
      continue;
    }
    const message = decodeProtobuf(frame.data, !whole);
    parts.push(message ? `${heading}\n${protoText(message, labels)}`.trimEnd() : `${heading}\n${labels.undecoded}`);
  }
  if (body.cut) parts.push(labels.more);
  return parts.join("\n\n");
}
