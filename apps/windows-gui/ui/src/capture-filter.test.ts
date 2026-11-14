import { describe, expect, it } from "vitest";
import type { CaptureRequest } from "./bridge";
import { combineFilter, MAX_REQUESTS, mergeRequests, nextSince, splitOr } from "./capture-filter";

function request(seq: number, done = true): CaptureRequest {
  return {
    seq,
    connection_id: 1,
    index: seq,
    method: "GET",
    url: `https://a.test/${seq}`,
    host: "a.test",
    status: done ? 200 : null,
    http_version: "HTTP/1.1",
    started_at_ms: seq,
    ended_at_ms: done ? seq + 1 : null,
    request_size: 0,
    response_size: 0,
  };
}

describe("capture filter", () => {
  it("adds quick filters to what was typed", () => {
    expect(combineFilter([], "  api ")).toBe("api");
    expect(combineFilter(["errors"], "")).toBe("status:>=400");
    expect(combineFilter(["json", "errors"], "host:a.test")).toBe("status:>=400 type:json host:a.test");
    expect(combineFilter(["errors"], "a OR b")).toBe("status:>=400 a OR status:>=400 b");
  });

  it("splits OR groups outside quotes only", () => {
    expect(splitOr("a OR b c")).toEqual(["a", "b c"]);
    expect(splitOr('header:"x OR y" OR z')).toEqual(['header:"x OR y"', "z"]);
    expect(splitOr("for ORDER")).toEqual(["for ORDER"]);
  });

  it("reads again from the oldest request still waiting", () => {
    expect(nextSince([request(3), request(4)], 4)).toBe(4);
    expect(nextSince([request(3), request(4, false), request(5, false)], 5)).toBe(3);
    expect(nextSince([], 0)).toBe(0);
  });

  it("merges answers, replacing updated requests and dropping pushed-out ones", () => {
    const rows = [request(1), request(2), request(3, false)];
    const merged = mergeRequests(rows, { requests: [request(3), request(4)], last_seq: 4, dropped_before_seq: 2 });
    expect(merged.map((row) => row.seq)).toEqual([2, 3, 4]);
    expect(merged[1].status).toBe(200);
    const many = Array.from({ length: MAX_REQUESTS + 5 }, (_, index) => request(index + 1));
    const capped = mergeRequests([], { requests: many });
    expect(capped).toHaveLength(MAX_REQUESTS);
    expect(capped[0].seq).toBe(6);
  });
});
