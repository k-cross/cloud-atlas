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

export const SNAPSHOT_VERSION = 3;

export interface SnapshotNode {
	id: number;
	key: string;
	label: string;
	kind: string;

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

export interface SnapshotObservation {
	key: string;
	last_seen: number;

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

	observations?: SnapshotObservation[];
	expired?: string[];
}

function edgeColor(kind: string): string {
	return EDGE_COLORS[kind] ?? DEFAULT_EDGE_COLOR;
}

function addNode(graph: Graph, node: SnapshotNode, x = 0, y = 0) {
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

	if (!graph.hasNode(edge.source_key) || !graph.hasNode(edge.target_key)) return;
	graph.addEdgeWithKey(edge.key, edge.source_key, edge.target_key, {
		kind: edge.kind,
		color: edgeColor(edge.kind),
		size: 1,
	});
}

function resize(graph: Graph, keys: Iterable<string>) {
	for (const key of keys) {
		if (graph.hasNode(key)) {
			graph.setNodeAttribute(key, "size", nodeSize(graph.degree(key)));
		}
	}
}

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

		graph.forEachNeighbor(key, (n) => {
			touched.add(n);
		});
		graph.dropNode(key);
		touched.delete(key);
	}

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
