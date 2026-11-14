import { describe, expect, it } from "vitest";
import type { CaptureConnection } from "./bridge";
import { asText, decodedBody, directionTotals, durationMs, fromBase64, header, headerLines, hexDump, looksLikeTls, noteText, parseHeaderLines, toBase64, withoutQuery } from "./capture-format";

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
});
