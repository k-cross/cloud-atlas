// Pure snapshot/patch → graphology translation, split out from main.ts so it
// can be unit-tested without a DOM or the wasm layout engine. main.ts owns the
// rendering/animation side; everything here is deterministic and side-effect
// free given its inputs.

import Graph from "graphology";
import {
  DEFAULT_EDGE_COLOR,
  EDGE_COLORS,
  PROVIDER_COLORS,
  nodeSize,
  providerOf,
} from "./style";

// Must match atlas-lib's `export::SNAPSHOT_VERSION` and atlas-layout's
// `graph::SNAPSHOT_VERSION`. v2 added the stable `key` fields the live backend
// uses to reference specific nodes/edges across rebuilds; graphology nodes are
// keyed by `key` so patches can locate them.
export const SNAPSHOT_VERSION = 2;

export interface SnapshotNode {
  id: number;
  key: string;
  label: string;
  kind: string;
}

export interface SnapshotEdge {
  source: number;
  target: number;
  key: string;
  source_key: string;
  target_key: string;
  kind: string;
}

export interface Snapshot {
  version: number;
  nodes: SnapshotNode[];
  edges: SnapshotEdge[];
}

export interface GraphPatch {
  version: number;
  added_nodes: SnapshotNode[];
  removed_nodes: string[];
  added_edges: SnapshotEdge[];
  removed_edges: string[];
}

function edgeColor(kind: string): string {
  return EDGE_COLORS[kind] ?? DEFAULT_EDGE_COLOR;
}

function addNode(graph: Graph, node: SnapshotNode) {
  // Idempotent: patch delivery is at-least-once, so re-adds must not throw.
  if (graph.hasNode(node.key)) return;
  graph.addNode(node.key, {
    label: node.label,
    kind: node.kind,
    color: PROVIDER_COLORS[providerOf(node.kind)],
    size: nodeSize(0),
    x: 0,
    y: 0,
  });
}

function addEdge(graph: Graph, edge: SnapshotEdge) {
  if (graph.hasEdge(edge.key)) return;
  // Endpoints should already exist; guard so an out-of-order patch is dropped
  // rather than crashing the stream.
  if (!graph.hasNode(edge.source_key) || !graph.hasNode(edge.target_key)) return;
  graph.addEdgeWithKey(edge.key, edge.source_key, edge.target_key, {
    kind: edge.kind,
    color: edgeColor(edge.kind),
    size: 1,
  });
}

// Degree-scaled sizing is recomputed for exactly the nodes a change touched,
// so hub/leaf sizes stay correct after incremental patches without rescanning
// the whole graph.
function resize(graph: Graph, keys: Iterable<string>) {
  for (const key of keys) {
    if (graph.hasNode(key)) {
      graph.setNodeAttribute(key, "size", nodeSize(graph.degree(key)));
    }
  }
}

export function buildGraph(snapshot: Snapshot): Graph {
  const graph = new Graph({ multi: true, type: "directed" });
  for (const node of snapshot.nodes) addNode(graph, node);
  for (const edge of snapshot.edges) addEdge(graph, edge);
  resize(
    graph,
    snapshot.nodes.map((n) => n.key),
  );
  return graph;
}

// Apply an incremental patch in dependency order — add nodes before the edges
// that reference them, and remove edges before the nodes they hang off — then
// refresh the sizes of every node the patch could have changed the degree of.
export function applyPatch(graph: Graph, patch: GraphPatch) {
  const touched = new Set<string>();

  for (const key of patch.removed_edges) {
    if (graph.hasEdge(key)) {
      graph.extremities(key).forEach((k) => touched.add(k));
      graph.dropEdge(key);
    }
  }
  for (const key of patch.removed_nodes) {
    if (!graph.hasNode(key)) continue;
    // Neighbors lose degree when this node (and its incident edges) go, so
    // record them for a resize before dropping.
    graph.forEachNeighbor(key, (n) => touched.add(n));
    graph.dropNode(key);
    touched.delete(key);
  }
  for (const node of patch.added_nodes) {
    addNode(graph, node);
    touched.add(node.key);
  }
  for (const edge of patch.added_edges) {
    addEdge(graph, edge);
    touched.add(edge.source_key);
    touched.add(edge.target_key);
  }

  resize(graph, touched);
}

// Serialize the current graphology state back into a snapshot the wasm layout
// engine can consume. Dense `id` is assigned in graphology iteration order,
// which is also the order the engine positions nodes in — keeping the position
// buffer aligned with `updateEachNodeAttributes` in main.ts after a patch.
export function snapshotFromGraph(graph: Graph): Snapshot {
  const idOf = new Map<string, number>();
  const nodes: SnapshotNode[] = graph.mapNodes((key, attrs) => {
    const id = idOf.size;
    idOf.set(key, id);
    return { id, key, label: attrs.label as string, kind: attrs.kind as string };
  });
  const edges: SnapshotEdge[] = graph.mapEdges(
    (key, attrs, source, target) => ({
      source: idOf.get(source)!,
      target: idOf.get(target)!,
      key,
      source_key: source,
      target_key: target,
      kind: attrs.kind as string,
    }),
  );
  return { version: SNAPSHOT_VERSION, nodes, edges };
}
