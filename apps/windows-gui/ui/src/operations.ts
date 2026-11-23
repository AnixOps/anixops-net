import type { ConnectPhase } from "./labels";

/**
 * The operations under way, oldest first, by name; one name can be there more than once. Each `run` adds its own
 * entry and takes away only that one when it ends, so an operation that ends first does not end another's progress.
 */
export type Operations = string[];

/** The connection switch's operations. They have their own lock: no other operation holds them up. */
const connectionOperations = new Set(["connect", "disconnect"]);

export function startOperation(operations: Operations, name: string): Operations {
  return [...operations, name];
}

/** `operations` without one entry of `name` (the one that ended). */
export function finishOperation(operations: Operations, name: string): Operations {
  const at = operations.lastIndexOf(name);
  return at < 0 ? operations : [...operations.slice(0, at), ...operations.slice(at + 1)];
}

/**
 * What the pages see as `busy`: the newest operation under way, or null. Page buttons wait while any operation runs;
 * the connection switch does not look at it (`toggleAction`).
 */
export function busyOperation(operations: Operations): string | null {
  return operations.length > 0 ? operations[operations.length - 1] : null;
}

/** The connection operation under way, for the switch's phase. */
export function connectionOperation(operations: Operations): "connect" | "disconnect" | null {
  for (let at = operations.length - 1; at >= 0; at -= 1) {
    const name = operations[at];
    if (connectionOperations.has(name)) return name as "connect" | "disconnect";
  }
  return null;
}

/**
 * What a press of the connection switch (or Ctrl+Shift+C) does in `phase`: connect, disconnect, or tell the user to
 * wait while the state is still being read or the connection is already changing. Other operations under way (a
 * delay test of every node, a subscription update) never hold it up.
 */
export function toggleAction(phase: ConnectPhase): "connect" | "disconnect" | "wait" {
  if (phase === "loading" || phase === "connecting" || phase === "disconnecting") return "wait";
  return phase === "on" ? "disconnect" : "connect";
}
