import { describe, expect, test } from "bun:test";
import Graph from "graphology";
import { buildGraph, SNAPSHOT_VERSION, type Snapshot } from "./graph";
import { CONFIRMED, INFERRED, SERVES_KIND, serviceEndpoints, serviceSummary } from "./service";
import { SERVES_STATUS_COLORS, servesColor, servesSize } from "./style";
import { observedFlows } from "./traffic";

function served(status: string | undefined): Graph {
	const snapshot: Snapshot = {
		version: SNAPSHOT_VERSION,
		nodes: [
			{ id: 0, key: "alb", label: "alb", kind: "AwsElbLoadBalancer" },
			{ id: 1, key: "i-1", label: "i-1", kind: "AwsEc2Instance" },
			{ id: 2, key: "10.0.0.1", label: "10.0.0.1", kind: "GenericIpAddress" },
			{ id: 3, key: "10.0.0.2", label: "10.0.0.2", kind: "GenericIpAddress" },
		],
		edges: [
			{
				source: 0,
				target: 1,
				key: "Serves|alb->i-1",
				source_key: "alb",
				target_key: "i-1",
				kind: SERVES_KIND,
			},
			{
				source: 2,
				target: 3,
				key: "TrafficFlow|10.0.0.1->10.0.0.2",
				source_key: "10.0.0.1",
				target_key: "10.0.0.2",
				kind: "TrafficFlow",
			},
		],
		observations: [
			{ key: "TrafficFlow|10.0.0.1->10.0.0.2", last_seen: 5, packets: 7, status: "accepted" },
			...(status === undefined ? [] : [{ key: "Serves|alb->i-1", last_seen: 0, status }]),
		],
	};
	return buildGraph(snapshot);
}

describe("serves styling", () => {
	test("confirmed reads as a path, inferred recedes", () => {
		expect(servesColor(CONFIRMED)).toBe(SERVES_STATUS_COLORS.confirmed);
		expect(servesSize(CONFIRMED)).toBeGreaterThan(servesSize(INFERRED));
	});

	test("an unobserved edge takes the dimmer reading, never the confirmed one", () => {
		expect(servesColor(undefined)).toBe(SERVES_STATUS_COLORS.inferred);
		expect(servesColor("something-else")).toBe(SERVES_STATUS_COLORS.inferred);
	});

	test("the two statuses are visually distinct", () => {
		expect(SERVES_STATUS_COLORS.confirmed).not.toBe(SERVES_STATUS_COLORS.inferred);
	});
});

describe("observed Serves edges", () => {
	test("take their color and width from the status, not from packets", () => {
		const graph = served(CONFIRMED);
		expect(graph.getEdgeAttribute("Serves|alb->i-1", "color")).toBe(servesColor(CONFIRMED));
		expect(graph.getEdgeAttribute("Serves|alb->i-1", "size")).toBe(servesSize(CONFIRMED));
		expect(graph.getEdgeAttribute("Serves|alb->i-1", "packets")).toBeUndefined();
	});

	test("an edge that arrives before its observation is not drawn as confirmed", () => {
		const graph = served(undefined);
		expect(graph.getEdgeAttribute("Serves|alb->i-1", "color")).toBe(servesColor(undefined));
	});
});

describe("the traffic layer", () => {
	// A Serves edge carries no packets, so animating beads along one would be
	// inventing a volume for it. The layer keys off the flow kind; this holds it
	// to that even though both kinds now carry an observation.
	test("never animates packets along a derived edge", () => {
		const flows = observedFlows(served(CONFIRMED));
		expect(flows.map((f) => f.key)).toEqual(["TrafficFlow|10.0.0.1->10.0.0.2"]);
	});
});

describe("serviceSummary", () => {
	test("counts the two provenances apart", () => {
		expect(serviceSummary(served(CONFIRMED))).toEqual({ confirmed: 1, inferred: 0 });
		expect(serviceSummary(served(INFERRED))).toEqual({ confirmed: 0, inferred: 1 });
	});

	test("an edge with no observation yet counts as inferred, never as confirmed", () => {
		expect(serviceSummary(served(undefined))).toEqual({ confirmed: 0, inferred: 1 });
	});
});

describe("serviceEndpoints", () => {
	test("keeps only what a Serves edge touches, so the plumbing can be hidden", () => {
		const keys = serviceEndpoints(served(CONFIRMED));
		expect([...keys].sort()).toEqual(["alb", "i-1"]);
		expect(keys.has("10.0.0.1")).toBe(false);
	});

	test("is empty when nothing has been derived", () => {
		const graph = new Graph({ multi: true, type: "directed" });
		graph.addNode("a", { kind: "AwsEc2Vpc" });
		expect(serviceEndpoints(graph).size).toBe(0);
	});
});
