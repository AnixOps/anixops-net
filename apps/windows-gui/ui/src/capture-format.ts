import type { CaptureConnection, CaptureDirection, CaptureMessage } from "./bridge";
import { t } from "./i18n";
import { formatBytes } from "./subscription-format";

/** The bytes of a base64 string from the engine. */
export function fromBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

/** Base64 of bytes, for the engine and the backend. */
export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return btoa(binary);
}

/** `Name: value` lines of headers, one per line, for a text box. */
export function headerLines(headers: [string, string][]): string {
  return headers.map(([name, value]) => `${name}: ${value}`).join("\n");
}

/** Headers from `Name: value` lines; blank lines and HTTP/2 pseudo-headers (`:path`) are left out. */
export function parseHeaderLines(text: string): [string, string][] {
  const headers: [string, string][] = [];
  for (const line of text.split(/\r?\n/)) {
    const colon = line.indexOf(":");
    if (colon <= 0) continue;
    const name = line.slice(0, colon).trim();
    if (name) headers.push([name, line.slice(colon + 1).trim()]);
  }
  return headers;
}

/** The URL without its query string and fragment. */
export function withoutQuery(url: string): string {
  const end = url.search(/[?#]/);
  return end < 0 ? url : url.slice(0, end);
}

/** UTF-8 text when the bytes are valid UTF-8 without control characters other than whitespace; null otherwise. */
export function asText(bytes: Uint8Array): string | null {
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
  return /[\u0000-\u0008\u000e-\u001f\u007f]/.test(text) ? null : text;
}

/** Offset, sixteen bytes in hex and their printable characters per line, like `xxd`. */
export function hexDump(bytes: Uint8Array, limit = 64 * 1024): string {
  const lines: string[] = [];
  const end = Math.min(bytes.length, limit);
  for (let offset = 0; offset < end; offset += 16) {
    const row = bytes.subarray(offset, Math.min(offset + 16, end));
    const hex = Array.from(row, (byte) => byte.toString(16).padStart(2, "0")).join(" ");
    const ascii = Array.from(row, (byte) => (byte >= 0x20 && byte < 0x7f ? String.fromCharCode(byte) : ".")).join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex.padEnd(47)}  ${ascii}`);
  }
  if (bytes.length > end) lines.push("…");
  return lines.join("\n");
}

/** A header's value, ignoring case; the first one when there are several. */
export function header(message: CaptureMessage, name: string): string | undefined {
  const wanted = name.toLowerCase();
  return message.headers.find(([key]) => key.toLowerCase() === wanted)?.[1];
}

/** Whether the first bytes of the upload are a TLS record (handshake, version 3.x), so the data is ciphertext. */
export function looksLikeTls(capture: CaptureConnection): boolean {
  const first = capture.chunks.find((chunk) => chunk.dir === "upload");
  if (!first) return false;
  const bytes = fromBase64(first.data_base64.slice(0, 8));
  return bytes.length >= 3 && bytes[0] === 0x16 && bytes[1] === 0x03;
}

/** "12 KB", or "64 KB of 1.2 MB kept" when capture stopped keeping bytes partway. */
export function directionTotals(direction: CaptureDirection): string {
  const kept = formatBytes(direction.captured);
  return direction.truncated ? t("已保留 {kept} / 共 {seen}", { kept, seen: formatBytes(direction.seen) }) : kept;
}

/** The size of a body, with how much of it was kept when it was cut. */
export function bodySize(body: { size: number; truncated: boolean; data_base64: string }): string {
  const size = formatBytes(body.size);
  return body.truncated ? t("{size}（保留前 {kept}）", { size, kept: formatBytes(fromBase64(body.data_base64).length) }) : size;
}

/** What the engine's `note` on an exchange means. */
export function noteText(note: string | null | undefined): string | null {
  switch (note) {
    case "answered_locally":
      return t("由改写规则直接应答");
    case "streaming":
      return t("流式响应，未保留内容");
    case "too_large":
      return t("内容过大，之后不再解密");
    default:
      return null;
  }
}

/** Milliseconds between two times, or null while the second is unknown. */
export function durationMs(startedAt: number, endedAt: number | null | undefined): number | null {
  return endedAt == null ? null : Math.max(0, endedAt - startedAt);
}

/** The local time of day of a Unix millisecond time, with seconds. */
export function timeOfDay(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour12: false });
}

/**
 * The body as the app saw it: gzip and deflate are undone (the webview's DecompressionStream), other encodings and
 * cut bodies are left as they are. Resolves to the bytes and whether they were decompressed.
 */
export async function decodedBody(bytes: Uint8Array, contentEncoding: string | undefined, truncated: boolean): Promise<{ bytes: Uint8Array; decoded: boolean }> {
  const encoding = contentEncoding?.trim().toLowerCase();
  if (truncated || (encoding !== "gzip" && encoding !== "deflate") || typeof DecompressionStream === "undefined") {
    return { bytes, decoded: false };
  }
  try {
    const stream = new Blob([bytes as BlobPart]).stream().pipeThrough(new DecompressionStream(encoding as CompressionFormat));
    return { bytes: new Uint8Array(await new Response(stream).arrayBuffer()), decoded: true };
  } catch {
    return { bytes, decoded: false };
  }
}

/** The capture status line: how many connections and how much of the budget they use. */
export function captureSummary(status: { connections: number; used_bytes: number; max_total_bytes: number }): string {
  return t("抓包中 · {count} 个连接 · {used} / {max}", {
    count: status.connections,
    used: formatBytes(status.used_bytes),
    max: formatBytes(status.max_total_bytes),
  });
}
