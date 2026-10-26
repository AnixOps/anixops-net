import type { NodeSummary } from "./bridge";

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
