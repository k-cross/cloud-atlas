import { describe, expect, test } from "bun:test";
import { type Snapshot, buildGraph } from "../src/graph";
import { PROVIDER_COLORS, nodeSize } from "../src/style";

function snapshot(partial: Partial<Snapshot>): Snapshot {
  return { version: 1, nodes: [], edges: [], ...partial };
}

describe("buildGraph", () => {
  test("maps every snapshot node and edge into the graph", () => {
    const graph = buildGraph(
      snapshot({
        nodes: [
          { id: 10, label: "vpc", kind: "AwsVpc" },
          { id: 20, label: "eni", kind: "AwsEni" },
          { id: 30, label: "subnet", kind: "AwsSubnet" },
        ],
        edges: [
          { source: 10, target: 30, kind: "Contains" },
          { source: 20, target: 30, kind: "AttachedTo" },
        ],
      }),
    );
    expect(graph.order).toBe(3);
    expect(graph.size).toBe(2);
  });

  test("re-keys sparse snapshot ids to dense node-list order", () => {
    // Position index i (from the layout engine) must line up with graph node
    // "i", regardless of the sparse snapshot ids.
    const graph = buildGraph(
      snapshot({
        nodes: [
          { id: 100, label: "a", kind: "AwsVpc" },
          { id: 7, label: "b", kind: "AwsSubnet" },
        ],
        edges: [{ source: 100, target: 7, kind: "Contains" }],
      }),
    );
    expect(graph.hasNode("0")).toBe(true);
    expect(graph.hasNode("1")).toBe(true);
    expect(graph.hasNode("100")).toBe(false);
    // The edge should connect the dense indices, not the raw ids.
    expect(graph.hasEdge("0", "1")).toBe(true);
    expect(graph.getNodeAttribute("0", "label")).toBe("a");
    expect(graph.getNodeAttribute("1", "label")).toBe("b");
  });

  test("colors nodes by provider bucket", () => {
    const graph = buildGraph(
      snapshot({
        nodes: [
          { id: 1, label: "aws", kind: "AwsEc2Instance" },
          { id: 2, label: "gcp", kind: "GcpComputeInstance" },
          { id: 3, label: "generic", kind: "GenericIpAddress" },
        ],
      }),
    );
    expect(graph.getNodeAttribute("0", "color")).toBe(PROVIDER_COLORS.AWS);
    expect(graph.getNodeAttribute("1", "color")).toBe(PROVIDER_COLORS.GCP);
    expect(graph.getNodeAttribute("2", "color")).toBe(PROVIDER_COLORS.Generic);
  });

  test("sizes nodes by their degree", () => {
    // node 0 has degree 2 (two incident edges), the leaves have degree 1.
    const graph = buildGraph(
      snapshot({
        nodes: [
          { id: 1, label: "hub", kind: "AwsVpc" },
          { id: 2, label: "leaf-a", kind: "AwsSubnet" },
          { id: 3, label: "leaf-b", kind: "AwsSubnet" },
        ],
        edges: [
          { source: 1, target: 2, kind: "Contains" },
          { source: 1, target: 3, kind: "Contains" },
        ],
      }),
    );
    expect(graph.getNodeAttribute("0", "size")).toBe(nodeSize(2));
    expect(graph.getNodeAttribute("1", "size")).toBe(nodeSize(1));
    expect(graph.getNodeAttribute("2", "size")).toBe(nodeSize(1));
  });

  test("isolated nodes keep the base size", () => {
    const graph = buildGraph(
      snapshot({ nodes: [{ id: 1, label: "lonely", kind: "AwsVpc" }] }),
    );
    expect(graph.getNodeAttribute("0", "size")).toBe(nodeSize(0));
  });

  test("is a directed multigraph — parallel edges are preserved, not deduped", () => {
    // The layout benefits from every edge as a spring; the frontend graph must
    // not collapse parallel edges the way GraphBuilder dedupes on the Rust side.
    const graph = buildGraph(
      snapshot({
        nodes: [
          { id: 1, label: "a", kind: "AwsEni" },
          { id: 2, label: "b", kind: "AwsSubnet" },
        ],
        edges: [
          { source: 1, target: 2, kind: "AttachedTo" },
          { source: 1, target: 2, kind: "RoutesTo" },
        ],
      }),
    );
    expect(graph.size).toBe(2);
    expect(graph.type).toBe("directed");
  });

  test("cross-cloud seam edges get their distinct color, unknown edges fall back", () => {
    const graph = buildGraph(
      snapshot({
        nodes: [
          { id: 1, label: "a", kind: "AwsEni" },
          { id: 2, label: "b", kind: "GenericIpAddress" },
        ],
        edges: [
          { source: 1, target: 2, kind: "RoutesTo" },
          { source: 2, target: 1, kind: "SomeFutureEdge" },
        ],
      }),
    );
    const colors = graph.mapEdges((_e, attr) => attr.color as string);
    expect(colors).toContain("#2f6285"); // EDGE_COLORS.RoutesTo
    expect(colors).toContain("#333b47"); // DEFAULT_EDGE_COLOR fallback
  });

  test("handles an empty snapshot", () => {
    const graph = buildGraph(snapshot({}));
    expect(graph.order).toBe(0);
    expect(graph.size).toBe(0);
  });
});
