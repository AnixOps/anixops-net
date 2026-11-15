import { describe, expect, it } from "vitest";
import { bodyModes, bodyShape, chosenMode, formPairs, imageType, mediaType, queryPairs, waterfall } from "./body-view";

describe("bodyShape", () => {
  it("reads the media type without parameters or case", () => {
    expect(mediaType("Application/JSON; charset=utf-8")).toBe("application/json");
    expect(mediaType(undefined)).toBe("");
  });

  it("recognises JSON by type, by suffix and by content", () => {
    expect(bodyShape("application/json", '{"a":1}').kind).toBe("json");
    expect(bodyShape("application/problem+json", '{"a":1}').kind).toBe("json");
    expect(bodyShape("text/plain", ' [1,2]').kind).toBe("json");
    expect(bodyShape(undefined, '{"a":1}').json).toEqual({ kind: "object", entries: [["a", { kind: "number", raw: "1" }]] });
  });

  it("shows JSON that does not parse, and other text, as text", () => {
    expect(bodyShape("application/json", '{"a":1').kind).toBe("text");
    expect(bodyShape("text/html", "<p>hi</p>").kind).toBe("text");
    expect(bodyShape("text/javascript", "[1,2]").kind).toBe("text");
    expect(bodyShape("text/plain", "hello").kind).toBe("text");
  });

  it("recognises images, forms and binary data", () => {
    expect(bodyShape("image/png", null).kind).toBe("image");
    expect(bodyShape("image/svg+xml; charset=utf-8", "<svg/>").kind).toBe("image");
    expect(bodyShape("application/x-www-form-urlencoded", "a=1").kind).toBe("form");
    expect(bodyShape("application/octet-stream", null).kind).toBe("binary");
    expect(bodyShape("image/tiff", null).kind).toBe("binary");
    expect(imageType("image/tiff")).toBeNull();
  });
});

describe("body modes", () => {
  it("offers the kind's default first and always ends with hex", () => {
    for (const kind of ["json", "image", "form", "text", "binary"] as const) {
      const modes = bodyModes(kind);
      expect(modes[modes.length - 1]).toBe("hex");
    }
    expect(bodyModes("json")[0]).toBe("tree");
  });

  it("keeps the user's last choice only where it applies", () => {
    expect(chosenMode("json", "pretty")).toBe("pretty");
    expect(chosenMode("json", "preview")).toBe("tree");
    expect(chosenMode("binary", "raw")).toBe("hex");
    expect(chosenMode("text", undefined)).toBe("raw");
  });
});

describe("pairs", () => {
  it("decodes form pairs, plus signs and bare names", () => {
    expect(formPairs("q=trail+shoes&page=2&flag&name=%E4%B8%AD%E6%96%87&=x&&")).toEqual([
      ["q", "trail shoes"], ["page", "2"], ["flag", ""], ["name", "中文"], ["", "x"],
    ]);
  });

  it("shows broken percent-encoding as written", () => {
    expect(formPairs("a=%E4%zz")).toEqual([["a", "%E4%zz"]]);
  });

  it("takes the query of a URL, without the fragment", () => {
    expect(queryPairs("https://api.example.com/v2/feed?cursor=a%3Db&limit=20#top")).toEqual([["cursor", "a=b"], ["limit", "20"]]);
    expect(queryPairs("https://api.example.com/v2/feed")).toEqual([]);
    expect(queryPairs("https://api.example.com/v2/feed?")).toEqual([]);
    // A question mark inside the fragment is not a query.
    expect(queryPairs("https://api.example.com/#/route?x=1")).toEqual([]);
  });
});

describe("waterfall", () => {
  it("places requests on the connection's time axis", () => {
    const bars = waterfall([
      { started_at_ms: 1000, ended_at_ms: 1500 },
      { started_at_ms: 1500, ended_at_ms: 2000 },
    ], 9999);
    expect(bars).toEqual([{ left: 0, width: 0.5, took: 500 }, { left: 0.5, width: 0.5, took: 500 }]);
  });

  it("runs an unfinished request to now and gives instant ones a visible bar", () => {
    const bars = waterfall([
      { started_at_ms: 0, ended_at_ms: 0 },
      { started_at_ms: 500, ended_at_ms: null },
    ], 1000);
    expect(bars[0]).toEqual({ left: 0, width: 0.01, took: 0 });
    expect(bars[1]).toEqual({ left: 0.5, width: 0.5, took: null });
  });

  it("keeps a bar inside the track and survives a single instant request", () => {
    expect(waterfall([{ started_at_ms: 5, ended_at_ms: 5 }], 5)).toEqual([{ left: 0, width: 0.01, took: 0 }]);
    const bars = waterfall([{ started_at_ms: 0, ended_at_ms: 1000 }, { started_at_ms: 1000, ended_at_ms: 1000 }], 1000);
    expect(bars[1].left + bars[1].width).toBeLessThanOrEqual(1 + 1e-9);
    expect(waterfall([], 0)).toEqual([]);
  });
});
