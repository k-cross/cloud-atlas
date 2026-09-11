import { describe, expect, test } from "bun:test";
import {
	applyPatch,
	buildGraph,
	type GraphPatch,
	SNAPSHOT_VERSION,
	type Snapshot,
	type SnapshotEdge,
	type SnapshotNode,
	type SnapshotObservation,
	snapshotFromGraph,
} from "./graph";
import { EDGE_COLORS, FLOW_STATUS_COLORS, flowSize, nodeSize, PROVIDER_COLORS } from "./style";

function node(key: string, kind: string, id = 0): SnapshotNode {
	return { id, key, label: key, kind };
}

function edge(sourceKey: string, targetKey: string, kind: string): SnapshotEdge {
	return {
		source: 0,
		target: 0,
		key: `${kind}|${sourceKey}->${targetKey}`,
		source_key: sourceKey,
		target_key: targetKey,
		kind,
	};
}

function observation(key: string, lastSeen: number, status = "accepted"): SnapshotObservation {
	return { key, last_seen: lastSeen, status };
}

function snapshot(partial: Partial<Snapshot>): Snapshot {
	return { version: SNAPSHOT_VERSION, nodes: [], edges: [], ...partial };
}

function patch(partial: Partial<GraphPatch>): GraphPatch {
	return {
		version: SNAPSHOT_VERSION,
		added_nodes: [],
		removed_nodes: [],
		added_edges: [],
		removed_edges: [],
		...partial,
	};
}

describe("buildGraph", () => {
	test("keys graphology nodes by stable snapshot key", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("vpc-1", "AwsEc2Vpc"), node("subnet-1", "AwsEc2Subnet")],
				edges: [edge("vpc-1", "subnet-1", "Contains")],
			}),
		);
		expect(graph.order).toBe(2);
		expect(graph.size).toBe(1);
		expect(graph.hasNode("vpc-1")).toBe(true);
		expect(graph.hasEdge("Contains|vpc-1->subnet-1")).toBe(true);
	});

	test("colors nodes by provider bucket", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [
					node("a", "AwsEc2Instance"),
					node("b", "GcpComputeInstance"),
					node("c", "GenericIpAddress"),
				],
			}),
		);
		expect(graph.getNodeAttribute("a", "color")).toBe(PROVIDER_COLORS.AWS);
		expect(graph.getNodeAttribute("b", "color")).toBe(PROVIDER_COLORS.GCP);
		expect(graph.getNodeAttribute("c", "color")).toBe(PROVIDER_COLORS.Generic);
	});

	test("sizes nodes by their degree", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("hub", "AwsEc2Vpc"), node("a", "AwsEc2Subnet"), node("b", "AwsEc2Subnet")],
				edges: [edge("hub", "a", "Contains"), edge("hub", "b", "Contains")],
			}),
		);
		expect(graph.getNodeAttribute("hub", "size")).toBe(nodeSize(2));
		expect(graph.getNodeAttribute("a", "size")).toBe(nodeSize(1));
	});

	test("is a directed multigraph — parallel edges are preserved", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("a", "AwsEc2Eni"), node("b", "AwsEc2Subnet")],
				edges: [edge("a", "b", "AttachedTo"), edge("a", "b", "RoutesTo")],
			}),
		);
		expect(graph.size).toBe(2);
		expect(graph.type).toBe("directed");
	});

	test("handles an empty snapshot", () => {
		const graph = buildGraph(snapshot({}));
		expect(graph.order).toBe(0);
		expect(graph.size).toBe(0);
	});
});

describe("applyPatch", () => {
	test("adds nodes and edges, then resizes affected nodes", () => {
		const graph = buildGraph(snapshot({ nodes: [node("hub", "AwsEc2Vpc")] }));
		applyPatch(
			graph,
			patch({
				added_nodes: [node("a", "AwsEc2Subnet"), node("b", "AwsEc2Subnet")],
				added_edges: [edge("hub", "a", "Contains"), edge("hub", "b", "Contains")],
			}),
		);
		expect(graph.order).toBe(3);
		expect(graph.size).toBe(2);
		expect(graph.getNodeAttribute("hub", "size")).toBe(nodeSize(2));
	});

	test("removes edges and nodes (and their incident edges)", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("hub", "AwsEc2Vpc"), node("a", "AwsEc2Subnet")],
				edges: [edge("hub", "a", "Contains")],
			}),
		);
		applyPatch(graph, patch({ removed_nodes: ["a"] }));
		expect(graph.hasNode("a")).toBe(false);
		expect(graph.size).toBe(0);
		expect(graph.getNodeAttribute("hub", "size")).toBe(nodeSize(0));
	});

	test("is idempotent for re-added nodes/edges (at-least-once delivery)", () => {
		const graph = buildGraph(snapshot({ nodes: [node("a", "AwsEc2Vpc")] }));
		const p = patch({
			added_nodes: [node("b", "AwsEc2Subnet")],
			added_edges: [edge("a", "b", "Contains")],
		});
		applyPatch(graph, p);
		applyPatch(graph, p);
		expect(graph.order).toBe(2);
		expect(graph.size).toBe(1);
	});

	test("drops an out-of-order edge whose endpoints are absent", () => {
		const graph = buildGraph(snapshot({ nodes: [node("a", "AwsEc2Vpc")] }));
		applyPatch(graph, patch({ added_edges: [edge("a", "missing", "Contains")] }));
		expect(graph.size).toBe(0);
	});

	test("seeds new nodes at the current centroid, not the origin", () => {
		const graph = buildGraph(
			snapshot({ nodes: [node("a", "AwsEc2Vpc"), node("b", "AwsEc2Subnet")] }),
		);
		graph.setNodeAttribute("a", "x", 100);
		graph.setNodeAttribute("a", "y", 40);
		graph.setNodeAttribute("b", "x", 200);
		graph.setNodeAttribute("b", "y", 60);

		applyPatch(graph, patch({ added_nodes: [node("c", "AwsEc2Subnet")] }));

		expect(graph.getNodeAttribute("c", "x")).toBeCloseTo(150);
		expect(graph.getNodeAttribute("c", "y")).toBeCloseTo(50);
	});
});

describe("snapshotFromGraph", () => {
	test("round-trips a built graph back to a consumable snapshot", () => {
		const original = snapshot({
			nodes: [node("a", "AwsEc2Vpc"), node("b", "AwsEc2Subnet")],
			edges: [edge("a", "b", "Contains")],
		});
		const rebuilt = snapshotFromGraph(buildGraph(original));
		expect(rebuilt.version).toBe(SNAPSHOT_VERSION);
		expect(rebuilt.nodes.map((n) => n.key)).toEqual(["a", "b"]);
		expect(rebuilt.nodes.map((n) => n.id)).toEqual([0, 1]);
		expect(rebuilt.edges[0]).toMatchObject({ source: 0, target: 1, kind: "Contains" });
		const again = buildGraph(rebuilt);
		expect(again.order).toBe(2);
		expect(again.size).toBe(1);
	});

	test("omits positions by default and carries them for a warm start", () => {
		const graph = buildGraph(snapshot({ nodes: [node("a", "AwsEc2Vpc")] }));
		graph.setNodeAttribute("a", "x", 12);
		graph.setNodeAttribute("a", "y", 34);

		const cold = snapshotFromGraph(graph);
		expect(cold.nodes[0].x).toBeUndefined();
		expect(cold.nodes[0].y).toBeUndefined();

		const warm = snapshotFromGraph(graph, true);
		expect(warm.nodes[0]).toMatchObject({ x: 12, y: 34 });
	});
});

describe("observations", () => {
	test("attach liveness to the node they name", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress")],
				observations: [observation("ip-a", 1788436800000)],
			}),
		);
		expect(graph.getNodeAttribute("ip-a", "lastSeen")).toBe(1788436800000);
		expect(graph.getNodeAttribute("ip-a", "flowStatus")).toBe("accepted");
	});

	test("attach liveness to the edge they name", () => {
		const flow = edge("ip-a", "ip-b", "TrafficFlow");
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress"), node("ip-b", "GenericIpAddress")],
				edges: [flow],
				observations: [observation(flow.key, 1788436800000, "mixed")],
			}),
		);
		expect(graph.getEdgeAttribute(flow.key, "flowStatus")).toBe("mixed");
	});

	test("naming nothing in the graph is ignored, not an error", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress")],
				observations: [observation("ip-nowhere", 1788436800000)],
			}),
		);
		expect(graph.order).toBe(1);
	});

	test("a patch refreshes freshness without touching topology", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress")],
				observations: [observation("ip-a", 1)],
			}),
		);
		applyPatch(graph, patch({ observations: [observation("ip-a", 2)] }));
		expect(graph.order).toBe(1);
		expect(graph.getNodeAttribute("ip-a", "lastSeen")).toBe(2);
	});

	test("an expired key stops reporting as live", () => {
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress")],
				observations: [observation("ip-a", 1)],
			}),
		);
		applyPatch(graph, patch({ expired: ["ip-a"] }));
		expect(graph.getNodeAttribute("ip-a", "lastSeen")).toBeUndefined();
		expect(graph.hasNode("ip-a")).toBe(true);
	});

	test("a patch with no liveness applies cleanly", () => {
		const graph = buildGraph(snapshot({ nodes: [node("ip-a", "GenericIpAddress")] }));
		applyPatch(graph, patch({ added_nodes: [node("ip-b", "GenericIpAddress")] }));
		expect(graph.order).toBe(2);
	});

	test("a flow edge takes its verdict's color and a volume-scaled width", () => {
		const flow = edge("ip-a", "ip-b", "TrafficFlow");
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress"), node("ip-b", "GenericIpAddress")],
				edges: [flow],
				observations: [{ key: flow.key, last_seen: 1, packets: 999, bytes: 1, status: "rejected" }],
			}),
		);
		expect(graph.getEdgeAttribute(flow.key, "color")).toBe(FLOW_STATUS_COLORS.rejected);
		expect(graph.getEdgeAttribute(flow.key, "size")).toBe(flowSize(999));
	});

	test("an expired flow edge goes back to its kind's plain styling", () => {
		const flow = edge("ip-a", "ip-b", "TrafficFlow");
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress"), node("ip-b", "GenericIpAddress")],
				edges: [flow],
				observations: [{ key: flow.key, last_seen: 1, packets: 999, bytes: 1, status: "accepted" }],
			}),
		);
		applyPatch(graph, patch({ expired: [flow.key] }));
		expect(graph.getEdgeAttribute(flow.key, "color")).toBe(EDGE_COLORS.TrafficFlow);
		expect(graph.getEdgeAttribute(flow.key, "size")).toBe(1);
	});

	test("volume rides on flow edges, not on nodes", () => {
		const flow = edge("ip-a", "ip-b", "TrafficFlow");
		const graph = buildGraph(
			snapshot({
				nodes: [node("ip-a", "GenericIpAddress"), node("ip-b", "GenericIpAddress")],
				edges: [flow],
				observations: [
					{ key: flow.key, last_seen: 1, packets: 24, bytes: 4800, status: "accepted" },
					observation("ip-a", 1),
				],
			}),
		);
		expect(graph.getEdgeAttribute(flow.key, "packets")).toBe(24);
		expect(graph.getNodeAttribute("ip-a", "packets")).toBeUndefined();
		expect(graph.getNodeAttribute("ip-a", "lastSeen")).toBe(1);
	});
});
