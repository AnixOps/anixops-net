import type { CaptureRequest, CaptureRequestsAnswer } from "./bridge";
import { t } from "./i18n";

/**
 * The capture budget this window asks for whenever it turns capture on: the engine's largest (16 MiB). The engine's
 * own default (2 MiB) is sized for the iOS Network Extension's memory limit; on a desktop, ordinary connections would
 * push decrypted requests out of it within seconds. A larger value is clamped by the engine.
 */
export const DESKTOP_CAPTURE_BUDGET = 16 * 1024 * 1024;

/** The `set_capture` request, always with the budget: the engine only changes the budget when one is given. */
export function setCaptureRequest(enabled: boolean, maxTotalBytes: number = DESKTOP_CAPTURE_BUDGET) {
  return { cmd: "set_capture", enabled, max_total_bytes: maxTotalBytes };
}

/**
 * Whether the engine behind the list is a new one: a changed setting, a node switch or a reconnect restarts it, and
 * its counters start again from 1, below what the list has read. The list then starts over, or the old cursor would
 * hide every new request and old rows would share keys with new ones.
 */
export function engineRestarted(cursor: { lastSeq: number; lastRev: number | null }, answer: CaptureRequestsAnswer): boolean {
  if (answer.last_rev != null && cursor.lastRev != null) return answer.last_rev < cursor.lastRev;
  return answer.last_seq != null && answer.last_seq < cursor.lastSeq;
}

/** Most requests the list keeps; older ones fall off the top. */
export const MAX_REQUESTS = 2000;

/** One-tap filters: each adds a condition of the engine's filter expression. */
export const quickFilters = [
  { id: "errors", label: () => t("出错"), condition: "status:>=400" },
  { id: "json", label: () => "JSON", condition: "type:json" },
  { id: "images", label: () => t("图片"), condition: "type:image" },
  { id: "h2", label: () => "HTTP/2", condition: "version:h2" },
  { id: "large", label: () => t("大于 100 KB"), condition: "size:>100k" },
] as const;

export type QuickFilter = (typeof quickFilters)[number]["id"];

/**
 * The expression sent to the engine: the quick filters' conditions and what the user typed, all of which must
 * hold. Typed `OR` groups each get the quick conditions, since the expression has no parentheses.
 */
export function combineFilter(quick: QuickFilter[], typed: string): string {
  const conditions = quickFilters.filter((filter) => quick.includes(filter.id)).map((filter) => filter.condition).join(" ");
  const text = typed.trim();
  if (!conditions) return text;
  if (!text) return conditions;
  return splitOr(text).map((group) => `${conditions} ${group}`).join(" OR ");
}

/** The `OR` groups of an expression, leaving `OR` inside double quotes alone. */
export function splitOr(text: string): string[] {
  const groups: string[] = [];
  let current = "";
  // A quoted value (closed or running to the end) is one token, so a quoted "OR" never splits.
  const words = text.match(/"[^"]*"?|[^\s"]+|\s+/g) ?? [];
  for (const word of words) {
    if (word === "OR") {
      groups.push(current.trim());
      current = "";
    } else {
      current += word;
    }
  }
  groups.push(current.trim());
  return groups.filter(Boolean);
}

/** A request's place in the capture (its connection and its place there): the same while it is under way and after. */
export function requestKey(request: Pick<CaptureRequest, "connection_id" | "index">): string {
  return `${request.connection_id}:${request.index}`;
}

/** Whether the engine may still fill in this request's response. */
function pending(request: CaptureRequest): boolean {
  if (request.state) return request.state === "in_progress";
  return request.status == null && request.ended_at_ms == null && !request.note;
}

/**
 * Where the next read starts, for kernels that list only by `seq`: before the oldest request still waiting for its
 * response, so it comes back with the response, or after the newest one seen.
 */
export function nextSince(rows: CaptureRequest[], lastSeq: number): number {
  const waiting = rows.find((row) => row.seq > 0 && pending(row));
  return waiting ? waiting.seq - 1 : lastSeq;
}

/**
 * The `since_rev` of the next read after `answer`: a full page carries on from its last row (the engine lists in
 * the order things changed), otherwise from the newest change. Null for a kernel without `since_rev`.
 */
export function nextRev(current: number | null, answer: CaptureRequestsAnswer, page: number): number | null {
  if (answer.last_rev == null) return null;
  const rows = answer.requests ?? [];
  const last = rows.length >= page ? rows[rows.length - 1].rev : undefined;
  return Math.max(current ?? 0, last ?? answer.last_rev);
}

/** The order rows are listed in: when they started, then by connection and place. */
export function compareArrival(a: CaptureRequest, b: CaptureRequest): number {
  return a.started_at_ms - b.started_at_ms || a.connection_id - b.connection_id || a.index - b.index;
}

/**
 * Whether `next` may replace `shown`, a copy of the same request: not when both carry the engine's change number and
 * `next` is older. An answer that arrives late never takes a row back to an earlier state.
 */
export function newerCopy(shown: CaptureRequest | undefined, next: CaptureRequest): boolean {
  if (!shown || shown.rev == null || next.rev == null) return true;
  return next.rev >= shown.rev;
}

/**
 * The list after one answer: newer copies of a request replace older ones (a request under way is updated in place
 * as its response arrives, and an older copy arriving late is ignored), oldest first.
 *
 * `dropped_before_seq` is not applied here. The engine drops whole connections, oldest connection first, and sets it
 * to the highest seq of a dropped connection; seqs are given as requests end, across all connections, so a
 * long-lived connection dropped first can carry the newest seq while requests of younger connections with smaller
 * seqs are still kept. Rows the engine no longer holds leave through `keepOnly` after a full read (`droppedMore`).
 */
export function mergeRequests(rows: CaptureRequest[], answer: CaptureRequestsAnswer): CaptureRequest[] {
  const byKey = new Map(rows.map((row) => [requestKey(row), row]));
  for (const request of answer.requests ?? []) {
    const key = requestKey(request);
    if (newerCopy(byKey.get(key), request)) byKey.set(key, request);
  }
  const merged = [...byKey.values()].sort(compareArrival);
  return merged.length > MAX_REQUESTS ? merged.slice(merged.length - MAX_REQUESTS) : merged;
}

/**
 * Whether the engine dropped something since `known` (the last `dropped_before_seq` the list saw): the budget pushed
 * out a connection, capture was cleared or a session was loaded. The list then reads everything the engine still
 * holds (`keepOnly`), since the number alone does not say which rows went.
 */
export function droppedMore(known: number, answer: CaptureRequestsAnswer): boolean {
  return (answer.dropped_before_seq ?? 0) > known;
}

/**
 * The rows the engine still holds after a full read of everything it keeps (`kept`, the keys of every row it
 * listed). Rows it no longer lists went with their connection, those still under way included.
 */
export function keepOnly(rows: CaptureRequest[], kept: Set<string>): CaptureRequest[] {
  return rows.filter((row) => kept.has(requestKey(row)));
}

/** Least time between two full reads while the budget keeps pushing connections out. */
export const RESYNC_INTERVAL_MS = 5000;

/**
 * The selected request as the inspector shows it: the row in the list, or, while that row is not in the list (a new
 * filter is being read, the list dropped it, the engine pushed it out), the copy kept from when it was. Only choosing
 * another request (or none) replaces the copy, so an open replay or mapping editor is not closed under the user.
 */
export function selectedRequest(
  rows: CaptureRequest[],
  selected: string | null,
  held: CaptureRequest | null,
): { request: CaptureRequest | null; missing: boolean } {
  if (selected == null) return { request: null, missing: false };
  const found = rows.find((row) => requestKey(row) === selected);
  if (found) return { request: found, missing: false };
  if (held && requestKey(held) === selected) return { request: held, missing: true };
  return { request: null, missing: false };
}

/** Most pages one full read takes; a capture holding more is read again later rather than without end. */
export const MAX_RESYNC_PAGES = 16;

/**
 * Reads every row the engine holds, a page at a time: by `since_rev` from 0 (the engine lists in the order things
 * changed, so a full page carries on after its last row), or by `since_seq` for a kernel without `rev`. `complete` is
 * false when a page failed or the read stopped after `maxPages`; the list then must not drop rows it did not see.
 */
export async function readAllKept(
  read: (sinceSeq: number, sinceRev: number) => Promise<CaptureRequestsAnswer>,
  page: number,
  maxPages: number = MAX_RESYNC_PAGES,
): Promise<{ answers: CaptureRequestsAnswer[]; complete: boolean }> {
  const answers: CaptureRequestsAnswer[] = [];
  let sinceSeq = 0;
  let sinceRev = 0;
  for (let count = 0; count < maxPages; count += 1) {
    const answer = await read(sinceSeq, sinceRev);
    if (answer.error) return { answers, complete: false };
    answers.push(answer);
    const rows = answer.requests ?? [];
    if (rows.length < page) return { answers, complete: true };
    const last = rows[rows.length - 1];
    if (answer.last_rev != null && last.rev != null) {
      sinceRev = last.rev;
    } else {
      sinceSeq = last.seq;
    }
  }
  return { answers, complete: false };
}
