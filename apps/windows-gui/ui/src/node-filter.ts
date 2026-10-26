import type { NodeDelay, NodeSummary } from "./bridge";
import { t } from "./i18n";

export function filterNodes(nodes: NodeSummary[], query: string, protocol: string): NodeSummary[] {
  const normalizedQuery = query.trim().toLowerCase();
  return nodes.filter((node) => {
    const searchable = `${node.label} ${node.protocol} ${node.group ?? ""}`.toLowerCase();
    return (!normalizedQuery || searchable.includes(normalizedQuery))
      && (protocol === "All" || node.protocol === protocol);
  });
}

export interface NodeSection {
  group?: string;
  nodes: NodeSummary[];
}

/** Nodes split by subscription, in the order the backend lists them. */
export function groupNodes(nodes: NodeSummary[]): NodeSection[] {
  const sections: NodeSection[] = [];
  for (const node of nodes) {
    const last = sections[sections.length - 1];
    if (last && last.group === node.group) last.nodes.push(node);
    else sections.push({ group: node.group, nodes: [node] });
  }
  return sections;
}

export type NodeSort = "default" | "name" | "delay";

/**
 * Nodes in the chosen order. By delay, answered nodes come first, fastest first, then failed ones, then untested
 * ones; ties keep the backend's order.
 */
export function sortNodes(nodes: NodeSummary[], sort: NodeSort, delays: Record<string, NodeDelay>): NodeSummary[] {
  if (sort === "default") return nodes;
  if (sort === "name") return [...nodes].sort((a, b) => a.label.localeCompare(b.label));
  const rank = (node: NodeSummary) => {
    const delay = delays[node.id];
    if (!delay) return Number.MAX_SAFE_INTEGER;
    return delay.delayMs ?? Number.MAX_SAFE_INTEGER - 1;
  };
  return [...nodes].sort((a, b) => rank(a) - rank(b));
}

/** `fast`, `slow` or `failed`, for the delay label's colour. */
export function delayTone(delay: NodeDelay): "fast" | "slow" | "failed" {
  if (delay.delayMs == null) return "failed";
  return delay.delayMs < 500 ? "fast" : "slow";
}

/** `123 ms`, or that the test failed. */
export function delayLabel(delay: NodeDelay): string {
  if (delay.delayMs == null) return t("失败");
  return `${delay.delayMs} ms`;
}
