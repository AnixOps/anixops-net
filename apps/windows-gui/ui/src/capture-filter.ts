import type { CaptureRequest, CaptureRequestsAnswer } from "./bridge";
import { t } from "./i18n";

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
 * as its response arrives, and an older copy arriving late is ignored), ended requests the engine dropped leave,
 * oldest first.
 */
export function mergeRequests(rows: CaptureRequest[], answer: CaptureRequestsAnswer): CaptureRequest[] {
  const byKey = new Map(rows.map((row) => [requestKey(row), row]));
  for (const request of answer.requests ?? []) {
    const key = requestKey(request);
    if (newerCopy(byKey.get(key), request)) byKey.set(key, request);
  }
  const dropped = answer.dropped_before_seq ?? 0;
  // A request still under way has no seq yet; if its connection went, it is simply not updated again.
  const merged = [...byKey.values()].filter((row) => row.seq === 0 || row.seq > dropped).sort(compareArrival);
  return merged.length > MAX_REQUESTS ? merged.slice(merged.length - MAX_REQUESTS) : merged;
}
