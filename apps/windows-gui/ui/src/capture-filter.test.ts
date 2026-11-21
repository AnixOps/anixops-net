import { describe, expect, it } from "vitest";
import type { CaptureRequest, CaptureRequestsAnswer } from "./bridge";
import { combineFilter, MAX_REQUESTS, mergeRequests, nextRev, nextSince, requestKey, splitOr } from "./capture-filter";

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
    // dropped_before_seq is the highest seq the engine dropped, so that row goes too.
    expect(merged.map((row) => row.seq)).toEqual([3, 4]);
    expect(merged[0].status).toBe(200);
    const many = Array.from({ length: MAX_REQUESTS + 5 }, (_, index) => request(index + 1));
    const capped = mergeRequests([], { requests: many });
    expect(capped).toHaveLength(MAX_REQUESTS);
    expect(capped[0].seq).toBe(6);
  });

  it("updates requests under way in place and keeps them when ended ones are dropped", () => {
    const live = (index: number, rev: number, patch: Partial<CaptureRequest> = {}): CaptureRequest => ({
      ...request(index, false),
      seq: 0,
      connection_id: 9,
      index,
      started_at_ms: 100 + index,
      rev,
      state: "in_progress",
      ...patch,
    });
    let rows = mergeRequests([], { requests: [live(0, 1)], last_seq: 0, last_rev: 1 });
    expect(rows.map(requestKey)).toEqual(["9:0"]);
    rows = mergeRequests(rows, {
      requests: [live(1, 2), live(0, 3, { status: 200, message_count: 4 })],
      last_seq: 0,
      last_rev: 3,
    });
    expect(rows.map(requestKey)).toEqual(["9:0", "9:1"]);
    expect(rows[0].status).toBe(200);
    expect(rows[0].message_count).toBe(4);
    rows = mergeRequests(rows, { requests: [live(0, 4, { seq: 1, state: "done", status: 200 })], last_seq: 1, last_rev: 4 });
    rows = mergeRequests(rows, { requests: [], last_seq: 1, last_rev: 4, dropped_before_seq: 1 });
    expect(rows.map(requestKey)).toEqual(["9:1"]);
    // A request under way has no seq, so the seq cursor ignores it.
    expect(nextSince(rows, 1)).toBe(1);
  });

  it("keeps the newest copy of a row whatever order the answers arrive in", () => {
    const copy = (rev: number, patch: Partial<CaptureRequest> = {}): CaptureRequest => ({
      ...request(0, false),
      seq: 0,
      connection_id: 9,
      index: 0,
      started_at_ms: 100,
      rev,
      state: "in_progress",
      ...patch,
    });
    const done: CaptureRequestsAnswer = { requests: [copy(4, { seq: 1, state: "done", status: 200 })], last_seq: 1, last_rev: 4 };
    const underWay: CaptureRequestsAnswer = { requests: [copy(3)], last_seq: 0, last_rev: 3 };
    const inOrder = mergeRequests(mergeRequests([], underWay), done);
    const late = mergeRequests(mergeRequests([], done), underWay);
    expect(late).toEqual(inOrder);
    expect(late[0].state).toBe("done");
    expect(late[0].rev).toBe(4);
  });

  it("moves the rev cursor to the newest change, or the last row of a full page", () => {
    const rows = [{ ...request(1), rev: 5 }, { ...request(2), rev: 7 }];
    expect(nextRev(null, { requests: rows, last_rev: 9 }, 2)).toBe(7);
    expect(nextRev(3, { requests: rows, last_rev: 9 }, 10)).toBe(9);
    expect(nextRev(12, { requests: [], last_rev: 9 }, 10)).toBe(12);
    expect(nextRev(null, { requests: rows, last_seq: 2 }, 10)).toBeNull();
  });
});

