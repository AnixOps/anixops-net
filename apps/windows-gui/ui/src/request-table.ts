import type { CaptureRequest } from "./bridge";
import { compareArrival } from "./capture-filter";

/**
 * How the request table is ordered: by one column, newest (or largest, slowest) first when descending. `seq` is the
 * time column: when requests started.
 */
export interface RequestSort {
  by: "seq" | "status" | "host" | "path" | "size" | "duration";
  descending: boolean;
}

/** Newest first: the order the table opens with. */
export const defaultSort: RequestSort = { by: "seq", descending: true };

/** The path and query of a request's URL, or the URL itself when it does not parse. */
export function requestPath(request: Pick<CaptureRequest, "url">): string {
  try {
    const url = new URL(request.url);
    return `${url.pathname}${url.search}`;
  } catch {
    return request.url;
  }
}

/**
 * The status column's color: green 2xx, amber 3xx, red 4xx/5xx or a request that ended without a response, and
 * neutral while the response is still on its way (or the engine answered it locally).
 */
export function statusTone(request: Pick<CaptureRequest, "status" | "ended_at_ms" | "note" | "state">): "success" | "warning" | "danger" | "pending" | "neutral" {
  const { status } = request;
  if (status == null) {
    if (request.note === "answered_locally") return "neutral";
    if (request.state === "in_progress") return "pending";
    return request.ended_at_ms == null && !request.note ? "pending" : "danger";
  }
  if (status >= 400) return "danger";
  if (status >= 300) return "warning";
  if (status >= 200) return "success";
  return "neutral";
}

function duration(request: CaptureRequest): number | null {
  return request.ended_at_ms == null ? null : request.ended_at_ms - request.started_at_ms;
}

/** Compares two requests for the table; ties and missing values keep the order they started in. */
export function compareRequests(a: CaptureRequest, b: CaptureRequest, sort: RequestSort): number {
  const direction = sort.descending ? -1 : 1;
  const bySeq = compareArrival(a, b) * direction;
  const missingLast = (x: number | null | undefined, y: number | null | undefined) => {
    if (x == null && y == null) return bySeq;
    if (x == null) return 1;
    if (y == null) return -1;
    return x === y ? bySeq : (x - y) * direction;
  };
  switch (sort.by) {
    case "seq":
      return bySeq;
    case "status":
      return missingLast(a.status, b.status);
    case "size":
      return missingLast(a.response_size, b.response_size);
    case "duration":
      return missingLast(duration(a), duration(b));
    case "host":
      return a.host.localeCompare(b.host) * direction || bySeq;
    case "path":
      return requestPath(a).localeCompare(requestPath(b)) * direction || bySeq;
  }
}
