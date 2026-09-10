// Pure snapshot/patch → graphology translation, split out from main.ts so it
// can be unit-tested without a DOM or the wasm layout engine. main.ts owns the
// rendering/animation side; everything here is deterministic and side-effect
// free given its inputs.

import Graph from "graphology";
import {
	DEFAULT_EDGE_COLOR,
	EDGE_COLORS,
	flowColor,
	flowSize,
	nodeSize,
	PROVIDER_COLORS,
	providerOf,
} from "./style";
import { FLOW_KIND } from "./traffic";

// Must match atlas-lib's `export::SNAPSHOT_VERSION` and atlas-layout's
// `graph::SNAPSHOT_VERSION`. v2 added the stable `key` fields the live backend
// uses to reference specific nodes/edges across rebuilds; graphology nodes are
// keyed by `key` so patches can locate them. v3 added `observations`, the
// Tier-2 liveness overlay, keyed by those same node/edge keys.
export const SNAPSHOT_VERSION = 3;

export interface SnapshotNode {
	id: number;
	key: string;
	label: string;
	kind: string;
	// Optional warm-start coordinates (present when reserializing the live graph
	// to re-seed the layout engine after a patch); absent on server snapshots.
	x?: number;
	y?: number;
}

export interface SnapshotEdge {
	source: number;
	target: number;
	key: string;
	source_key: string;
	target_key: string;
	kind: string;
}

// Liveness for one node or edge, keyed by the same stable `key` it carries.
// A key naming nothing in the graph is ignored: the flow feed can observe a
// resource before any scan has found it, and inventing a node for it is the
// server's job, not ours.
export interface SnapshotObservation {
	key: string;
	last_seen: number;
	// Volume is present on flow edges only. A node omits it: there it would be
	// an undirected sum over every flow that touched the node, and one record
	// names several nodes, so it would count the same traffic more than once.
	// Sum a node's incident TrafficFlow edges instead.
	packets?: number;
	bytes?: number;
	status: string;
}

export interface Snapshot {
	version: number;
	nodes: SnapshotNode[];
	edges: SnapshotEdge[];
	observations?: SnapshotObservation[];
}

export interface GraphPatch {
	version: number;
	added_nodes: SnapshotNode[];
	removed_nodes: string[];
	added_edges: SnapshotEdge[];
	removed_edges: string[];
	// Freshness that changed, and keys whose observation lapsed. Both are
	// optional so a server that predates the overlay still applies cleanly.
	observations?: SnapshotObservation[];
	expired?: string[];
}

function edgeColor(kind: string): string {
	return EDGE_COLORS[kind] ?? DEFAULT_EDGE_COLOR;
}

function addNode(graph: Graph, node: SnapshotNode, x = 0, y = 0) {
	// Idempotent: patch delivery is at-least-once, so re-adds must not throw.
	if (graph.hasNode(node.key)) return;
	graph.addNode(node.key, {
		label: node.label,
		kind: node.kind,
		color: PROVIDER_COLORS[providerOf(node.kind)],
		size: nodeSize(0),
		x,
		y,
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

// Liveness lives on the graphology entity itself, so a renderer can style by
// freshness without a second lookup structure to keep in sync with patches.
// Observations arrive on their own cadence — far more often than topology
// changes — which is why they travel as their own list rather than as fields
// on the node.
function observe(graph: Graph, observations: SnapshotObservation[]) {
	for (const o of observations) {
		const attrs: Record<string, unknown> = {
			lastSeen: o.last_seen,
			flowStatus: o.status,
		};
		if (o.packets !== undefined) attrs.packets = o.packets;
		if (o.bytes !== undefined) attrs.bytes = o.bytes;
		if (graph.hasNode(o.key)) {
			graph.mergeNodeAttributes(o.key, attrs);
		} else if (graph.hasEdge(o.key)) {
			if (graph.getEdgeAttribute(o.key, "kind") === FLOW_KIND) {
				attrs.color = flowColor(o.status);
				attrs.size = flowSize(o.packets);
			}
			graph.mergeEdgeAttributes(o.key, attrs);
		}
	}
}

// A lapsed observation has to be cleared, not just left stale: a client that
// keeps the last freshness it heard shows a silent resource as live forever.
function clearObservations(graph: Graph, keys: string[]) {
	const attrs: Record<string, unknown> = {
		lastSeen: undefined,
		packets: undefined,
		bytes: undefined,
		flowStatus: undefined,
	};
	for (const key of keys) {
		if (graph.hasNode(key)) {
			graph.mergeNodeAttributes(key, attrs);
		} else if (graph.hasEdge(key)) {
			const kind = graph.getEdgeAttribute(key, "kind") as string;
			graph.mergeEdgeAttributes(key, { ...attrs, color: edgeColor(kind), size: 1 });
		}
	}
}

export function buildGraph(snapshot: Snapshot): Graph {
	const graph = new Graph({ multi: true, type: "directed" });
	for (const node of snapshot.nodes) addNode(graph, node);
	for (const edge of snapshot.edges) addEdge(graph, edge);
	observe(graph, snapshot.observations ?? []);
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
			for (const k of graph.extremities(key)) touched.add(k);
			graph.dropEdge(key);
		}
	}
	for (const key of patch.removed_nodes) {
		if (!graph.hasNode(key)) continue;
		// Neighbors lose degree when this node (and its incident edges) go, so
		// record them for a resize before dropping.
		graph.forEachNeighbor(key, (n) => {
			touched.add(n);
		});
		graph.dropNode(key);
		touched.delete(key);
	}
	// Seed newcomers at the current centroid so the warm-started layout grows
	// them out from inside the existing cloud rather than from the origin.
	const [cx, cy] = centroid(graph);
	for (const node of patch.added_nodes) {
		addNode(graph, node, cx, cy);
		touched.add(node.key);
	}
	for (const edge of patch.added_edges) {
		addEdge(graph, edge);
		touched.add(edge.source_key);
		touched.add(edge.target_key);
	}

	// After the topology, so an observation about something the same patch
	// added lands on an entity that now exists.
	clearObservations(graph, patch.expired ?? []);
	observe(graph, patch.observations ?? []);

	resize(graph, touched);
}

function centroid(graph: Graph): [number, number] {
	let sx = 0;
	let sy = 0;
	let n = 0;
	graph.forEachNode((_k, a) => {
		const x = a.x as number;
		const y = a.y as number;
		if (Number.isFinite(x) && Number.isFinite(y)) {
			sx += x;
			sy += y;
			n += 1;
		}
	});
	return n > 0 ? [sx / n, sy / n] : [0, 0];
}

// Serialize the current graphology state back into a snapshot the wasm layout
// engine can consume. Dense `id` is assigned in graphology iteration order,
// which is also the order the engine positions nodes in — keeping the position
// buffer aligned with `updateEachNodeAttributes` in main.ts after a patch.
//
// `withPositions` carries each node's current coordinates through as a
// warm-start seed; the engine PINS those nodes (they exert forces but never
// move) and lays out only the position-less newcomers. Used for patches so an
// update can never re-flow the existing cloud; a cold (re)build omits them.
export function snapshotFromGraph(graph: Graph, withPositions = false): Snapshot {
	const idOf = new Map<string, number>();
	const nodes: SnapshotNode[] = graph.mapNodes((key, attrs) => {
		const id = idOf.size;
		idOf.set(key, id);
		const node: SnapshotNode = {
			id,
			key,
			label: attrs.label as string,
			kind: attrs.kind as string,
		};
		if (withPositions) {
			node.x = attrs.x as number;
			node.y = attrs.y as number;
		}
		return node;
	});
	const edges: SnapshotEdge[] = graph.mapEdges((key, attrs, source, target) => ({
		source: idOf.get(source)!,
		target: idOf.get(target)!,
		key,
		source_key: source,
		target_key: target,
		kind: attrs.kind as string,
	}));
	return { version: SNAPSHOT_VERSION, nodes, edges };
}
