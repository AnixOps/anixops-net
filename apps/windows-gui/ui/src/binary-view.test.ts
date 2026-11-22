import { describe, expect, it } from "vitest";
import {
  decodeProtobuf,
  drawable,
  grpcFrames,
  grpcText,
  imageDimensions,
  isGrpcType,
  isProtobufType,
  looksLikeProtobuf,
  PROTO_MAX_DEPTH,
  PROTO_MAX_FIELDS,
  ProtoLabels,
  protoText,
  sniffImage,
} from "./binary-view";
import { bodyModes, bodyShape, opensElsewhere } from "./body-view";

const bytes = (...values: Array<number | string | number[]>): Uint8Array => {
  const out: number[] = [];
  for (const value of values) {
    const items = typeof value === "string" ? Array.from(new TextEncoder().encode(value)) : Array.isArray(value) ? value : [value];
    for (const item of items) out.push(item);
  }
  return Uint8Array.from(out);
};

const labels: ProtoLabels = {
  message: (index, size) => `message ${index} (${size})`,
  compressed: "compressed",
  trailers: "trailers",
  undecoded: "not protobuf",
  more: "…",
};

const decoded = (data: Uint8Array, cut = false) => {
  const message = decodeProtobuf(data, cut);
  return message && protoText(message, labels);
};

// The same cases are in CaptureBinaryTests.swift and CaptureBinaryTest.kt: the three must print the same text.
const sample = bytes(
  [0x08, 0x96, 0x01], // 1: 150
  [0x12, 0x07], "testing", // 2: "testing"
  [0x1a, 0x03, 0x08, 0x96, 0x01], // 3 { 1: 150 }
  [0x21, 0x18, 0x2d, 0x44, 0x54, 0xfb, 0x21, 0x09, 0x40], // 4: fixed64
  [0x2d, 0x00, 0x00, 0x80, 0x3f], // 5: fixed32
  [0x32, 0x03, 0x00, 0xff, 0x10], // 6: bytes that are neither text nor a message
  [0x38, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01], // 7: -1 as int64
  [0x42, 0x00], // 8: empty
  [0x4a, 0x05], 'a"b\n\\', // 9: text that needs escapes
);
const sampleText = [
  "1: 150",
  '2: "testing"',
  "3 {",
  "  1: 150",
  "}",
  "4: 0x400921fb54442d18 (fixed64)",
  "5: 0x3f800000 (fixed32)",
  "6: <00 ff 10> (3 B)",
  "7: 18446744073709551615 (-1)",
  '8: ""',
  '9: "a\\"b\\n\\\\"',
].join("\n");

describe("Protobuf without a schema", () => {
  it("prints every wire type the way protoc --decode_raw does", () => {
    expect(decoded(sample)).toBe(sampleText);
  });

  it("shows long bytes by their start and size", () => {
    const blob = Uint8Array.from({ length: 40 }, (_, index) => (index === 0 ? 0 : 0x80 + index));
    expect(decoded(bytes([0x0a, 40], Array.from(blob)))).toBe(
      `1: <${Array.from(blob.subarray(0, 32), (byte) => byte.toString(16).padStart(2, "0")).join(" ")} …> (40 B)`,
    );
  });

  it("refuses bytes that are not a whole message", () => {
    expect(decodeProtobuf(bytes(0x08))).toBeNull(); // varint cut off
    expect(decodeProtobuf(bytes(0x0a, 0x05, 0x01))).toBeNull(); // length past the end
    expect(decodeProtobuf(bytes(0x0f, 0x00))).toBeNull(); // wire type 7
    expect(decodeProtobuf(bytes(0x0b, 0x0c))).toBeNull(); // groups
    expect(decodeProtobuf(bytes(0x00, 0x01))).toBeNull(); // field 0
    expect(decodeProtobuf(bytes([0x08], Array(11).fill(0xff), [0x01]))).toBeNull(); // varint over ten bytes
    expect(decodeProtobuf(bytes([0x08], Array(9).fill(0xff), [0x02]))).toBeNull(); // over 64 bits
  });

  it("keeps the fields before the cut of a body capture cut short", () => {
    expect(decoded(bytes([0x08, 0x96, 0x01, 0x12, 0x07], "tes"))).toBeNull();
    expect(decoded(bytes([0x08, 0x96, 0x01, 0x12, 0x07], "tes"), true)).toBe("1: 150\n…");
  });

  it("is an empty message when there are no bytes, but never offered for unknown binary data", () => {
    expect(decodeProtobuf(new Uint8Array())).toEqual({ fields: [], cut: false });
    expect(looksLikeProtobuf(new Uint8Array())).toBeNull();
    expect(looksLikeProtobuf(sample)).not.toBeNull();
    expect(looksLikeProtobuf(bytes(0xff, 0xfe))).toBeNull();
  });

  it("opens messages inside messages only so deep", () => {
    let inner = bytes(0x08, 0x01);
    for (let level = 0; level < PROTO_MAX_DEPTH + 4; level += 1) inner = bytes([0x0a, inner.length], Array.from(inner));
    const text = decoded(inner) ?? "";
    expect(text.split("\n").filter((line) => line.trimEnd().endsWith("{"))).toHaveLength(PROTO_MAX_DEPTH - 1);
    expect(text).toContain("B)");
  });

  it("stops at the field limit and says so", () => {
    const many = new Uint8Array((PROTO_MAX_FIELDS + 500) * 2);
    for (let index = 0; index < many.length; index += 2) {
      many[index] = 0x08;
      many[index + 1] = 0x01;
    }
    const message = decodeProtobuf(many);
    expect(message?.fields).toHaveLength(PROTO_MAX_FIELDS);
    expect(message?.cut).toBe(true);
    // The limit counts nested fields too, and still marks the body as cut when it is reached inside a message.
    const nested = bytes([0x0a, 0xa0, 0x9c, 0x01], Array.from(many.subarray(0, 20_000)), [0x10, 0x01]);
    const inner = decodeProtobuf(nested);
    expect(inner?.cut).toBe(false);
    const over = bytes([0x0a, 0xd0, 0xb8, 0x02], Array.from(many.subarray(0, 40_016)));
    expect(decodeProtobuf(over)?.cut).toBe(true);
  });

  it("knows the media types", () => {
    expect(isProtobufType("application/x-protobuf")).toBe(true);
    expect(isProtobufType("application/vnd.foo+proto")).toBe(true);
    expect(isProtobufType("application/grpc+proto")).toBe(false);
    expect(isGrpcType("application/grpc")).toBe(true);
    expect(isGrpcType("application/grpc-web+proto")).toBe(true);
    expect(isGrpcType("application/grpc-web-text")).toBe(false);
  });
});

describe("gRPC frames", () => {
  const frame = (flag: number, data: Uint8Array) => bytes([flag, 0, 0, (data.length >> 8) & 0xff, data.length & 0xff], Array.from(data));

  it("decodes each message and shows gRPC-Web trailers", () => {
    const body = bytes(Array.from(frame(0, bytes(0x08, 0x96, 0x01))), Array.from(frame(1, bytes(1, 2, 3))), Array.from(frame(0x80, bytes("grpc-status:0\r\n"))));
    const frames = grpcFrames(body);
    expect(frames?.frames).toHaveLength(3);
    expect(grpcText(frames!, labels)).toBe("# message 1 (3)\n1: 150\n\n# message 2 (3)\ncompressed\n\n# trailers\ngrpc-status:0");
  });

  it("keeps a message cut short and refuses what is not framed", () => {
    const cut = frame(0, bytes(0x08, 0x96, 0x01, 0x10, 0x02)).subarray(0, 9);
    expect(grpcText(grpcFrames(cut)!, labels)).toBe("# message 1 (5)\n1: 150\n…\n\n…");
    expect(grpcFrames(bytes(0x02, 0, 0, 0, 0))).toBeNull();
    expect(grpcFrames(bytes(0, 0, 0))).toBeNull();
    expect(grpcText(grpcFrames(frame(0, bytes(0xff)))!, labels)).toBe("# message 1 (1)\nnot protobuf");
  });
});

describe("pictures", () => {
  const png = (width: number, height: number) => bytes([0x89], "PNG\r\n\u001a\n", [0, 0, 0, 13], "IHDR", [
    (width >>> 24) & 0xff, (width >> 16) & 0xff, (width >> 8) & 0xff, width & 0xff,
    (height >>> 24) & 0xff, (height >> 16) & 0xff, (height >> 8) & 0xff, height & 0xff,
  ]);

  it("recognises pictures by their first bytes", () => {
    expect(sniffImage(png(1, 1))).toBe("image/png");
    expect(sniffImage(bytes(0xff, 0xd8, 0xff, 0xe0))).toBe("image/jpeg");
    expect(sniffImage(bytes("GIF89a"))).toBe("image/gif");
    expect(sniffImage(bytes("RIFF", [0, 0, 0, 0], "WEBPVP8 "))).toBe("image/webp");
    expect(sniffImage(bytes([0, 0, 0, 0x1c], "ftypavif"))).toBe("image/avif");
    expect(sniffImage(bytes("BM", Array(12).fill(0), [40, 0, 0, 0], Array(8).fill(0)))).toBe("image/bmp");
    expect(sniffImage(bytes("BMx"))).toBeNull();
    expect(sniffImage(bytes("hello"))).toBeNull();
  });

  it("reads the size from the header and refuses to draw huge ones", () => {
    expect(imageDimensions(png(640, 480), "image/png")).toEqual({ width: 640, height: 480 });
    expect(drawable(imageDimensions(png(65535, 65535), "image/png"))).toBe(false);
    expect(drawable(imageDimensions(png(8064, 6048), "image/png"))).toBe(true);
    expect(imageDimensions(bytes("GIF89a", [0x10, 0, 0x20, 0]), "image/gif")).toEqual({ width: 16, height: 32 });
    const jpeg = bytes([0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0x01, 0xe0, 0x02, 0x80]);
    expect(imageDimensions(jpeg, "image/jpeg")).toEqual({ width: 640, height: 480 });
    const vp8x = bytes("RIFF", [0, 0, 0, 0], "WEBPVP8X", [10, 0, 0, 0, 0, 0, 0, 0, 0x7f, 0x02, 0, 0xdf, 0x01, 0]);
    expect(imageDimensions(vp8x, "image/webp")).toEqual({ width: 640, height: 480 });
    expect(imageDimensions(bytes([0x89], "PNG"), "image/png")).toBeNull();
    expect(drawable(null)).toBe(true);
  });
});

describe("body shapes for binary data", () => {
  it("decodes Protobuf by type and offers it for unknown binary data that reads as a message", () => {
    expect(bodyShape("application/x-protobuf", null, sample).kind).toBe("protobuf");
    expect(bodyShape("application/grpc", null, bytes(0, 0, 0, 0, 3, 0x08, 0x96, 0x01)).grpc?.frames).toHaveLength(1);
    const unknown = bodyShape("application/octet-stream", null, sample);
    expect(unknown.kind).toBe("binary");
    expect(bodyModes(unknown.kind, Boolean(unknown.proto))).toEqual(["hex", "protobuf"]);
    expect(bodyShape("application/x-protobuf", null, bytes(0xff, 0xff)).kind).toBe("binary");
  });

  it("shows pictures sent as generic bytes", () => {
    expect(bodyShape("application/octet-stream", null, bytes(0xff, 0xd8, 0xff, 0xe0))).toEqual({ kind: "image", image: "image/jpeg" });
    expect(bodyShape(undefined, null, bytes("GIF89a", [0, 0x80]))).toEqual({ kind: "image", image: "image/gif" });
  });

  it("opens only types that run no code in another app", () => {
    expect(opensElsewhere("image/png")).toBe(true);
    expect(opensElsewhere("application/problem+json")).toBe(true);
    expect(opensElsewhere("image/svg+xml")).toBe(false);
    expect(opensElsewhere("text/html; charset=utf-8")).toBe(false);
    expect(opensElsewhere("application/octet-stream")).toBe(false);
    expect(opensElsewhere(undefined)).toBe(false);
  });
});

/** A small seeded generator, so a failure prints a seed that reproduces it. */
function random(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

describe("untrusted bytes", () => {
  const types = ["image/png", "image/jpeg", "image/gif", "image/webp", "image/bmp", "image/avif"];
  const seeds = [
    sample,
    bytes(0, 0, 0, 0, 3, 0x08, 0x96, 0x01, 0x80, 0, 0, 0, 2, 0x61, 0x62),
    bytes([0x89], "PNG\r\n\u001a\n", [0, 0, 0, 13], "IHDR"),
    bytes([0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0x01, 0xe0, 0x02, 0x80]),
    bytes("RIFF", [0, 0, 0, 0], "WEBPVP8L", Array(12).fill(0xff)),
  ];

  it("never throws on random or damaged input, and keeps the output bounded", () => {
    const next = random(20261122);
    for (let round = 0; round < 4000; round += 1) {
      let data: Uint8Array;
      if (round % 2 === 0) {
        data = Uint8Array.from({ length: Math.floor(next() * 600) }, () => Math.floor(next() * 256));
      } else {
        // A known shape with a few bytes changed, cut or repeated: closer to what breaks parsers.
        const base = seeds[round % seeds.length];
        data = Uint8Array.from(base);
        for (let flips = 0; flips < 1 + Math.floor(next() * 4); flips += 1) data[Math.floor(next() * data.length)] = Math.floor(next() * 256);
        if (next() < 0.3) data = data.subarray(0, Math.floor(next() * data.length));
        if (next() < 0.2) data = bytes(Array.from(data), Array.from(data));
      }
      const context = `round ${round}: ${Array.from(data, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
      expect(() => {
        const message = decodeProtobuf(data, next() < 0.5);
        if (message) expect(protoText(message, labels).length).toBeLessThan(200 * (data.length + 1));
        looksLikeProtobuf(data);
        const frames = grpcFrames(data);
        if (frames) grpcText(frames, labels);
        sniffImage(data);
        for (const type of types) {
          const size = imageDimensions(data, type);
          if (size) expect(size.width >= 0 && size.height >= 0).toBe(true);
        }
        bodyShape("application/octet-stream", null, data, true);
      }, context).not.toThrow();
    }
  });

  it("stays quick on a large body of messages nested as deep as they go", () => {
    // Bytes that pass the text check until the end, wrapped in as many messages as are opened: each level is
    // looked at again, which the scan limit keeps to a fixed total.
    let inner: Uint8Array = new Uint8Array(2 * 1024 * 1024 - 128).fill(0x80);
    for (let level = 0; level < PROTO_MAX_DEPTH + 2; level += 1) {
      const prefix = [0x0a];
      for (let rest = inner.length; ; rest = Math.floor(rest / 128)) {
        prefix.push(rest < 128 ? rest : (rest & 0x7f) | 0x80);
        if (rest < 128) break;
      }
      const next = new Uint8Array(prefix.length + inner.length);
      next.set(prefix);
      next.set(inner, prefix.length);
      inner = next;
    }
    const started = performance.now();
    const message = decodeProtobuf(inner);
    expect(message?.fields).toHaveLength(1);
    expect(performance.now() - started).toBeLessThan(4000);
  });
});
