// Pure snapshot → graphology translation, split out from main.ts so it can be
// unit-tested without a DOM or the wasm layout engine. main.ts owns the
// rendering/animation side; everything here is deterministic and side-effect
// free given a snapshot.

import Graph from "graphology";
import {
  DEFAULT_EDGE_COLOR,
  EDGE_COLORS,
  PROVIDER_COLORS,
  nodeSize,
  providerOf,
} from "./style";

// Must match atlas-lib's `export::SNAPSHOT_VERSION` and atlas-layout's
// `graph::SNAPSHOT_VERSION`.
export const SNAPSHOT_VERSION = 1;

export interface SnapshotNode {
  id: number;
  label: string;
  kind: string;
}

export interface SnapshotEdge {
  source: number;
  target: number;
  kind: string;
}

export interface Snapshot {
  version: number;
  nodes: SnapshotNode[];
  edges: SnapshotEdge[];
}

export function buildGraph(snapshot: Snapshot): Graph {
  // Snapshot ids may be sparse; both the layout engine and this graph use
  // dense node-list order, so position index i belongs to graph node "i".
  const indexOf = new Map<number, number>();
  snapshot.nodes.forEach((node, i) => indexOf.set(node.id, i));

  const degrees = new Array<number>(snapshot.nodes.length).fill(0);
  for (const edge of snapshot.edges) {
    degrees[indexOf.get(edge.source)!] += 1;
    degrees[indexOf.get(edge.target)!] += 1;
  }

  const graph = new Graph({ multi: true, type: "directed" });
  snapshot.nodes.forEach((node, i) => {
    graph.addNode(i, {
      label: node.label,
      kind: node.kind,
      color: PROVIDER_COLORS[providerOf(node.kind)],
      size: nodeSize(degrees[i]),
      x: 0,
      y: 0,
    });
  });
  for (const edge of snapshot.edges) {
    graph.addEdge(indexOf.get(edge.source)!, indexOf.get(edge.target)!, {
      kind: edge.kind,
      color: EDGE_COLORS[edge.kind] ?? DEFAULT_EDGE_COLOR,
      size: 1,
    });
  }
  return graph;
}
