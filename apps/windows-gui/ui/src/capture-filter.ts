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

/** Whether the engine may still fill in this request's response. */
function pending(request: CaptureRequest): boolean {
  return request.status == null && request.ended_at_ms == null && !request.note;
}

/**
 * Where the next read starts: before the oldest request still waiting for its response, so it comes back with
 * the response, or after the newest one seen.
 */
export function nextSince(rows: CaptureRequest[], lastSeq: number): number {
  const waiting = rows.find(pending);
  return waiting ? waiting.seq - 1 : lastSeq;
}

/** The list after one answer: newer copies replace older ones, pushed-out requests leave, oldest first. */
export function mergeRequests(rows: CaptureRequest[], answer: CaptureRequestsAnswer): CaptureRequest[] {
  const bySeq = new Map(rows.map((row) => [row.seq, row]));
  for (const request of answer.requests ?? []) bySeq.set(request.seq, request);
  const dropped = answer.dropped_before_seq ?? 0;
  const merged = [...bySeq.values()].filter((row) => row.seq > dropped).sort((a, b) => a.seq - b.seq);
  return merged.length > MAX_REQUESTS ? merged.slice(merged.length - MAX_REQUESTS) : merged;
}
