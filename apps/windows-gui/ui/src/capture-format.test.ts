import { describe, expect, it } from "vitest";
import type { CaptureConnection } from "./bridge";
import {
  appendMessages,
  asText,
  decodedBody,
  directionTotals,
  durationMs,
  frameDetail,
  fromBase64,
  header,
  headerLines,
  hexDump,
  looksLikeTls,
  messageText,
  messageTitle,
  notableFrame,
  noteText,
  parseHeaderLines,
  skippedText,
  streamKind,
  toBase64,
  withoutQuery,
} from "./capture-format";
import type { H2Frame, StreamMessage } from "./bridge";

const base64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));

async function gzip(text: string): Promise<Uint8Array> {
  const stream = new Blob([text]).stream().pipeThrough(new CompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

function connection(firstUpload: Uint8Array): CaptureConnection {
  return {
    id: 1,
    network: "tcp",
    host: "example.com",
    destination: "example.com:443",
    started_at_ms: 0,
    ended_at_ms: null,
    decrypted: false,
    upload: { seen: firstUpload.length, captured: firstUpload.length, truncated: false },
    download: { seen: 0, captured: 0, truncated: false },
    chunks: [{ dir: "upload", at_ms: 1, data_base64: base64(firstUpload) }],
    http: [],
  };
}

describe("capture format", () => {
  it("encodes base64 that decodes back", () => {
    const bytes = new Uint8Array(70000).map((_, index) => index % 256);
    expect(toBase64(new Uint8Array([102, 111]))).toBe("Zm8=");
    expect(Array.from(fromBase64(toBase64(bytes)))).toEqual(Array.from(bytes));
  });

  it("edits headers as lines", () => {
    expect(headerLines([["Accept", "*/*"], ["X-A", "1:2"]])).toBe("Accept: */*\nX-A: 1:2");
    expect(parseHeaderLines("Accept: */*\r\n\n:path: /\nX-A: 1:2\nbroken")).toEqual([["Accept", "*/*"], ["X-A", "1:2"]]);
  });

  it("drops the query of a URL", () => {
    expect(withoutQuery("https://a.test/p?q=1#f")).toBe("https://a.test/p");
    expect(withoutQuery("https://a.test/p")).toBe("https://a.test/p");
  });

  it("decodes base64 and tells text from binary", () => {
    expect(Array.from(fromBase64("AP/+"))).toEqual([0, 255, 254]);
    expect(asText(new TextEncoder().encode("GET / HTTP/1.1\r\nHost: a\r\n\r\n"))).toContain("Host: a");
    expect(asText(new TextEncoder().encode("你好"))).toBe("你好");
    expect(asText(new Uint8Array([0x16, 0x03, 0x01, 0x00]))).toBeNull();
    expect(asText(new Uint8Array([0xff, 0xfe]))).toBeNull();
  });

  it("dumps hex with offsets and printable characters", () => {
    const dump = hexDump(new TextEncoder().encode("ABCDEFGHIJKLMNOPQ"));
    expect(dump.split("\n")).toEqual([
      "00000000  41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f 50  ABCDEFGHIJKLMNOP",
      "00000010  51                                               Q",
    ]);
    expect(hexDump(new Uint8Array(40), 16).endsWith("…")).toBe(true);
  });

  it("finds headers without regard to case", () => {
    const message = { headers: [["Content-Type", "text/html"], ["content-encoding", "gzip"]] as Array<[string, string]>, body: { size: 0, truncated: false, data_base64: "" } };
    expect(header(message, "content-type")).toBe("text/html");
    expect(header(message, "Content-Encoding")).toBe("gzip");
    expect(header(message, "Cookie")).toBeUndefined();
  });

  it("recognises a TLS client hello", () => {
    expect(looksLikeTls(connection(new Uint8Array([0x16, 0x03, 0x01, 0x02, 0x00])))).toBe(true);
    expect(looksLikeTls(connection(new TextEncoder().encode("GET / HTTP/1.1")))).toBe(false);
  });

  it("says how much was kept and what notes mean", () => {
    expect(directionTotals({ seen: 2048, captured: 2048, truncated: false })).toBe("2.00 KB");
    expect(directionTotals({ seen: 4096, captured: 2048, truncated: true })).toContain("4.00 KB");
    expect(noteText("answered_locally")).toBe("由改写规则直接应答");
    expect(noteText(null)).toBeNull();
    expect(durationMs(100, 350)).toBe(250);
    expect(durationMs(100, null)).toBeNull();
  });

  it("undoes gzip but leaves cut or unknown encodings alone", async () => {
    const zipped = await gzip("hello capture");
    const decoded = await decodedBody(zipped, "gzip", false);
    expect(decoded.decoded).toBe(true);
    expect(new TextDecoder().decode(decoded.bytes)).toBe("hello capture");
    expect((await decodedBody(zipped, "gzip", true)).decoded).toBe(false);
    expect((await decodedBody(zipped, "br", false)).decoded).toBe(false);
  });

  it("asks the kernel first and falls back when it cannot decode", async () => {
    const seen: Array<[string, string, boolean]> = [];
    const kernel = async (data: string, encoding: string, complete: boolean) => {
      seen.push([data, encoding, complete]);
      return { data_base64: toBase64(new TextEncoder().encode("from kernel")), truncated: !complete };
    };
    const zstd = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);
    const decoded = await decodedBody(zstd, "zstd", true, kernel);
    expect(new TextDecoder().decode(decoded.bytes)).toBe("from kernel");
    expect(decoded).toMatchObject({ decoded: true, partial: true });
    expect(seen).toEqual([[toBase64(zstd), "zstd", false]]);
    // Nothing to undo: the kernel is not asked.
    expect((await decodedBody(zstd, "identity", false, kernel)).decoded).toBe(false);
    expect(seen).toHaveLength(1);
    // The kernel refuses: gzip of a whole body still comes out through the webview.
    const zipped = await gzip("hello capture");
    const fallback = await decodedBody(zipped, "gzip", false, async () => {
      throw new Error("corrupt");
    });
    expect(new TextDecoder().decode(fallback.bytes)).toBe("hello capture");
  });

  it("names streams, messages and frames", () => {
    const row = { method: "GET", status: 101, response_content_type: null, message_count: 0 };
    expect(streamKind(row)).toBe("WS");
    expect(streamKind({ ...row, status: 200, response_content_type: "text/event-stream; charset=utf-8" })).toBe("SSE");
    expect(streamKind({ ...row, status: 200 })).toBeNull();
    expect(skippedText("too_large")).toContain("1 MiB");
    expect(skippedText("streaming")).toContain("流式");

    const message = (index: number, patch: Partial<StreamMessage> = {}): StreamMessage => ({
      index, at_ms: 0, dir: "down", kind: "sse", size: 2, truncated: false, text: "hi", ...patch,
    });
    expect(messageTitle(message(0, { label: "delta" }))).toBe("delta");
    expect(messageTitle(message(0))).toBe("message");
    expect(messageTitle(message(0, { kind: "close", code: 1000 }))).toBe("Close 1000");
    expect(messageText(message(0, { text: null, data_base64: "AAE=" }))).toContain("00 01");

    const first = appendMessages([], [message(2), message(3)]);
    expect(first.messages.map((item) => item.index)).toEqual([2, 3]);
    expect(first.missed).toBe(2);
    const again = appendMessages(first.messages, [message(3), message(4)]);
    expect(again.messages.map((item) => item.index)).toEqual([2, 3, 4]);
    expect(again.missed).toBe(0);
    expect(appendMessages(again.messages, [message(5)], 2).messages.map((item) => item.index)).toEqual([4, 5]);

    const frame = (patch: Partial<H2Frame>): H2Frame => ({
      index: 0, at_ms: 0, link: "app", dir: "down", type: "DATA", flag_names: [], stream: 1, length: 0, ...patch,
    });
    expect(frameDetail(frame({ type: "WINDOW_UPDATE", value: 65535 }))).toBe("+65535");
    expect(frameDetail(frame({ type: "RST_STREAM", error: "CANCEL" }))).toBe("CANCEL");
    expect(frameDetail(frame({}))).toBeNull();
    expect(notableFrame(frame({ type: "GOAWAY", value: 3 }))).toBe(true);
    expect(notableFrame(frame({}))).toBe(false);
  });
});

