import { describe, expect, it } from "vitest";
import type { CaptureRequest } from "./bridge";
import { compareRequests, defaultSort, requestPath, statusTone } from "./request-table";

function request(seq: number, patch: Partial<CaptureRequest> = {}): CaptureRequest {
  return {
    seq,
    connection_id: 1,
    index: seq,
    method: "GET",
    url: `https://a.test/${seq}?q=1`,
    host: "a.test",
    status: 200,
    http_version: "HTTP/1.1",
    started_at_ms: 1000,
    ended_at_ms: 1000 + seq,
    request_size: 0,
    response_size: seq * 10,
    ...patch,
  };
}

const order = (rows: CaptureRequest[], by: Parameters<typeof compareRequests>[2]) => [...rows].sort((a, b) => compareRequests(a, b, by)).map((row) => row.seq);

describe("request table", () => {
  it("opens newest first", () => {
    expect(order([request(1), request(3), request(2)], defaultSort)).toEqual([3, 2, 1]);
  });

  it("sorts by size and duration, missing values last either way", () => {
    const rows = [request(1), request(2, { ended_at_ms: null }), request(3)];
    expect(order(rows, { by: "duration", descending: true })).toEqual([3, 1, 2]);
    expect(order(rows, { by: "duration", descending: false })).toEqual([1, 3, 2]);
    expect(order(rows, { by: "size", descending: true })).toEqual([3, 2, 1]);
  });

  it("sorts by status with pending requests last and ties by seq", () => {
    const rows = [request(1, { status: 404 }), request(2, { status: null }), request(3, { status: 200 }), request(4, { status: 200 })];
    expect(order(rows, { by: "status", descending: false })).toEqual([3, 4, 1, 2]);
  });

  it("sorts by host and path", () => {
    const rows = [request(1, { host: "b.test" }), request(2, { host: "a.test" })];
    expect(order(rows, { by: "host", descending: false })).toEqual([2, 1]);
    expect(order([request(2), request(1)], { by: "path", descending: false })).toEqual([1, 2]);
  });

  it("shows the path with its query", () => {
    expect(requestPath(request(7))).toBe("/7?q=1");
    expect(requestPath({ url: "not a url" })).toBe("not a url");
  });

  it("colors the status", () => {
    expect(statusTone(request(1))).toBe("success");
    expect(statusTone(request(1, { status: 302 }))).toBe("warning");
    expect(statusTone(request(1, { status: 500 }))).toBe("danger");
    expect(statusTone(request(1, { status: null, ended_at_ms: null }))).toBe("pending");
    expect(statusTone(request(1, { status: null }))).toBe("danger");
    expect(statusTone(request(1, { status: null, note: "answered_locally" }))).toBe("neutral");
  });
});
