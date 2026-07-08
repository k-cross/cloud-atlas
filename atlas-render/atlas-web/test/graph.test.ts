import { describe, expect, test } from "bun:test";
import {
  type GraphPatch,
  SNAPSHOT_VERSION,
  type Snapshot,
  type SnapshotEdge,
  type SnapshotNode,
  applyPatch,
  buildGraph,
  snapshotFromGraph,
} from "../src/graph";
import { PROVIDER_COLORS, nodeSize } from "../src/style";

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
    expect(graph.size).toBe(0); // incident edge went with it
    expect(graph.getNodeAttribute("hub", "size")).toBe(nodeSize(0));
  });

  test("is idempotent for re-added nodes/edges (at-least-once delivery)", () => {
    const graph = buildGraph(snapshot({ nodes: [node("a", "AwsEc2Vpc")] }));
    const p = patch({
      added_nodes: [node("b", "AwsEc2Subnet")],
      added_edges: [edge("a", "b", "Contains")],
    });
    applyPatch(graph, p);
    applyPatch(graph, p); // replay must not throw or duplicate
    expect(graph.order).toBe(2);
    expect(graph.size).toBe(1);
  });

  test("drops an out-of-order edge whose endpoints are absent", () => {
    const graph = buildGraph(snapshot({ nodes: [node("a", "AwsEc2Vpc")] }));
    applyPatch(graph, patch({ added_edges: [edge("a", "missing", "Contains")] }));
    expect(graph.size).toBe(0);
  });

  test("seeds new nodes at the current centroid, not the origin", () => {
    // Warm-start relies on newcomers spawning inside the existing cloud so the
    // layout grows them out locally instead of flinging them from (0,0).
    const graph = buildGraph(
      snapshot({ nodes: [node("a", "AwsEc2Vpc"), node("b", "AwsEc2Subnet")] }),
    );
    graph.setNodeAttribute("a", "x", 100);
    graph.setNodeAttribute("a", "y", 40);
    graph.setNodeAttribute("b", "x", 200);
    graph.setNodeAttribute("b", "y", 60);

    applyPatch(graph, patch({ added_nodes: [node("c", "AwsEc2Subnet")] }));

    expect(graph.getNodeAttribute("c", "x")).toBeCloseTo(150); // centroid of a,b
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
    // Dense ids are assigned in iteration order and edges reference them.
    expect(rebuilt.nodes.map((n) => n.id)).toEqual([0, 1]);
    expect(rebuilt.edges[0]).toMatchObject({ source: 0, target: 1, kind: "Contains" });
    // A rebuild from the round-tripped snapshot is structurally identical.
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
