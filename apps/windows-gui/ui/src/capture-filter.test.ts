import { describe, expect, it } from "vitest";
import type { CaptureRequest, CaptureRequestsAnswer } from "./bridge";
import {
  combineFilter,
  DESKTOP_CAPTURE_BUDGET,
  droppedMore,
  engineRestarted,
  keepOnly,
  MAX_REQUESTS,
  mergeRequests,
  nextRev,
  nextSince,
  readAllKept,
  requestKey,
  selectedRequest,
  setCaptureRequest,
  splitOr,
} from "./capture-filter";

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

  it("merges answers, replacing updated requests, and leaves dropping to a full read", () => {
    const rows = [request(1), request(2), request(3, false)];
    const merged = mergeRequests(rows, { requests: [request(3), request(4)], last_seq: 4, dropped_before_seq: 2 });
    // dropped_before_seq alone does not say which rows went.
    expect(merged.map((row) => row.seq)).toEqual([1, 2, 3, 4]);
    expect(merged[2].status).toBe(200);
    expect(droppedMore(0, { requests: [], dropped_before_seq: 2 })).toBe(true);
    expect(droppedMore(2, { requests: [], dropped_before_seq: 2 })).toBe(false);
    expect(droppedMore(2, { requests: [] })).toBe(false);
    const kept = keepOnly(merged, new Set([requestKey(request(3)), requestKey(request(4))]));
    expect(kept.map((row) => row.seq)).toEqual([3, 4]);
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
    // The engine dropped row 9:0 alone (a full read lists only 9:1): the row under way stays.
    rows = keepOnly(mergeRequests(rows, { requests: [], last_seq: 1, last_rev: 4, dropped_before_seq: 1 }), new Set(["9:1"]));
    expect(rows.map(requestKey)).toEqual(["9:1"]);
    // A request under way has no seq, so the seq cursor ignores it.
    expect(nextSince(rows, 1)).toBe(1);
  });

  it("keeps the rows of younger connections when a long-lived connection with the newest seq is dropped", () => {
    const on = (connection: number, seq: number, patch: Partial<CaptureRequest> = {}): CaptureRequest => ({
      ...request(seq),
      connection_id: connection,
      index: seq,
      started_at_ms: seq,
      rev: seq,
      state: "done",
      ...patch,
    });
    // Connection 5 (HTTP/2, opened first) carries seqs 10, 200 and 950; connections 6..8 carry 11..13; connection 6
    // also has a request under way.
    const longLived = [on(5, 10), on(5, 200), on(5, 950)];
    const younger = [on(6, 11), on(7, 12), on(8, 13)];
    const underWay = on(5, 0, { index: 3, started_at_ms: 960, rev: 960, state: "in_progress", status: null, ended_at_ms: null });
    let rows = mergeRequests([], { requests: [...longLived, ...younger, underWay], last_seq: 950, last_rev: 960 });
    // The engine drops connection 5: dropped_before_seq becomes 950, above every kept row's seq.
    const answer: CaptureRequestsAnswer = { requests: [], last_seq: 950, last_rev: 960, dropped_before_seq: 950 };
    rows = mergeRequests(rows, answer);
    expect(droppedMore(0, answer)).toBe(true);
    expect(rows.map((row) => row.seq).sort((a, b) => a - b)).toEqual([0, 10, 11, 12, 13, 200, 950]);
    // The full read lists only what the engine holds: connection 5's rows go, the request under way with them.
    rows = keepOnly(rows, new Set(younger.map(requestKey)));
    expect(rows.map(requestKey)).toEqual(["6:11", "7:12", "8:13"]);
  });

  it("reads everything kept a page at a time, by rev or by seq", async () => {
    const page = (seqs: number[], rev = true): CaptureRequestsAnswer => ({
      requests: seqs.map((seq) => ({ ...request(seq), ...(rev ? { rev: seq * 10 } : {}) })),
      last_seq: 9,
      ...(rev ? { last_rev: 90 } : {}),
    });
    const calls: Array<[number, number]> = [];
    const pages = [page([1, 2]), page([3, 4]), page([5])];
    const full = await readAllKept(async (sinceSeq, sinceRev) => {
      calls.push([sinceSeq, sinceRev]);
      return pages[calls.length - 1];
    }, 2);
    expect(full.complete).toBe(true);
    expect(full.answers).toHaveLength(3);
    expect(calls).toEqual([[0, 0], [0, 20], [0, 40]]);

    const bySeq: Array<[number, number]> = [];
    const old = [page([1, 2], false), page([], false)];
    const oldFull = await readAllKept(async (sinceSeq, sinceRev) => {
      bySeq.push([sinceSeq, sinceRev]);
      return old[bySeq.length - 1];
    }, 2);
    expect(oldFull.complete).toBe(true);
    expect(bySeq).toEqual([[0, 0], [2, 0]]);

    // A read that never ends a page, or fails, is not complete: no row may be dropped on it.
    const endless = await readAllKept(async () => page([1, 2]), 2, 3);
    expect(endless).toMatchObject({ complete: false });
    expect(endless.answers).toHaveLength(3);
    const failed = await readAllKept(async () => ({ error: { code: "invalid_filter" } }), 2);
    expect(failed.complete).toBe(false);
  });

  it("keeps the selected request while it is out of the list, until another is chosen", () => {
    const shown = { ...request(3), connection_id: 4 };
    expect(selectedRequest([shown], "4:3", null)).toEqual({ request: shown, missing: false });
    // A new filter is being read (or the row was dropped): the held copy stays, marked as missing.
    expect(selectedRequest([], "4:3", shown)).toEqual({ request: shown, missing: true });
    // Another request chosen, or none: the held copy does not stand in for it.
    expect(selectedRequest([], "4:9", shown)).toEqual({ request: null, missing: false });
    expect(selectedRequest([shown], null, shown)).toEqual({ request: null, missing: false });
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


describe("setCaptureRequest", () => {
  it("always asks for the desktop budget, not the engine's mobile default", () => {
    expect(DESKTOP_CAPTURE_BUDGET).toBe(16 * 1024 * 1024);
    expect(setCaptureRequest(true)).toEqual({ cmd: "set_capture", enabled: true, max_total_bytes: 16 * 1024 * 1024 });
    expect(setCaptureRequest(false)).toEqual({ cmd: "set_capture", enabled: false, max_total_bytes: 16 * 1024 * 1024 });
    expect(setCaptureRequest(true, 4096)).toMatchObject({ max_total_bytes: 4096 });
  });
});

describe("engineRestarted", () => {
  it("notices counters that went back, which only a new engine does", () => {
    expect(engineRestarted({ lastSeq: 40, lastRev: 120 }, { requests: [], last_seq: 2, last_rev: 3 })).toBe(true);
    expect(engineRestarted({ lastSeq: 40, lastRev: 120 }, { requests: [], last_seq: 40, last_rev: 120 })).toBe(false);
    expect(engineRestarted({ lastSeq: 40, lastRev: 120 }, { requests: [], last_seq: 41, last_rev: 125 })).toBe(false);
    // Nothing read yet: nothing to compare.
    expect(engineRestarted({ lastSeq: 0, lastRev: null }, { requests: [], last_seq: 0, last_rev: 0 })).toBe(false);
  });

  it("falls back to seq for a kernel without rev", () => {
    expect(engineRestarted({ lastSeq: 40, lastRev: null }, { requests: [], last_seq: 3 })).toBe(true);
    expect(engineRestarted({ lastSeq: 40, lastRev: null }, { requests: [], last_seq: 40 })).toBe(false);
    expect(engineRestarted({ lastSeq: 40, lastRev: null }, { requests: [] })).toBe(false);
  });
});
