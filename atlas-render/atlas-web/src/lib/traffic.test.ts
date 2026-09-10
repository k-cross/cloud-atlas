import { describe, expect, test } from "bun:test";
import { buildGraph, SNAPSHOT_VERSION, type Snapshot } from "./graph";
import { FLOW_PACKET_COLORS, FLOW_STATUS_COLORS } from "./style";
import {
	FRESHNESS_FADE_MS,
	formatAge,
	formatBytes,
	freshness,
	MAX_PACKETS_PER_EDGE,
	MIN_PACKETS_PER_EDGE,
	observedFlows,
	packetCount,
	packetPositions,
	trafficSummary,
	transitMs,
} from "./traffic";

const flowKey = "TrafficFlow|ip-a->ip-b";

function flowGraph(observations: Snapshot["observations"]) {
	return {
		graph: buildGraph({
			version: SNAPSHOT_VERSION,
			nodes: [
				{ id: 0, key: "ip-a", label: "10.0.0.1", kind: "GenericIpAddress" },
				{ id: 1, key: "ip-b", label: "10.0.0.2", kind: "GenericIpAddress" },
				{ id: 2, key: "vpc", label: "vpc-1", kind: "AwsEc2Vpc" },
			],
			edges: [
				{
					source: 0,
					target: 1,
					key: flowKey,
					source_key: "ip-a",
					target_key: "ip-b",
					kind: "TrafficFlow",
				},
				{
					source: 2,
					target: 0,
					key: "Contains|vpc->ip-a",
					source_key: "vpc",
					target_key: "ip-a",
					kind: "Contains",
				},
			],
			observations,
		}),
	};
}

describe("observedFlows", () => {
	test("reports only flow edges an observation has landed on", () => {
		const { graph } = flowGraph([
			{ key: flowKey, last_seen: 10, packets: 128, bytes: 16384, status: "accepted" },
			{ key: "ip-a", last_seen: 10, status: "accepted" },
		]);
		const flows = observedFlows(graph);
		expect(flows.length).toBe(1);
		expect(flows[0]).toMatchObject({
			key: flowKey,
			source: "ip-a",
			target: "ip-b",
			status: "accepted",
			packets: 128,
			bytes: 16384,
			color: FLOW_STATUS_COLORS.accepted,
			beadColor: FLOW_PACKET_COLORS.accepted,
		});
	});

	test("a flow edge with no observation yet is not drawn as traffic", () => {
		const { graph } = flowGraph([]);
		expect(observedFlows(graph).length).toBe(0);
	});
});

describe("trafficSummary", () => {
	test("totals volume across flows and counts nodes heard from", () => {
		const { graph } = flowGraph([
			{ key: flowKey, last_seen: 20, packets: 128, bytes: 16384, status: "rejected" },
			{ key: "ip-a", last_seen: 15, status: "accepted" },
			{ key: "ip-b", last_seen: 20, status: "rejected" },
		]);
		const summary = trafficSummary(graph);
		expect(summary).toMatchObject({
			flows: 1,
			packets: 128,
			bytes: 16384,
			liveNodes: 2,
			newest: 20,
		});
		expect(summary.byStatus.rejected).toBe(1);
	});

	test("an unobserved graph summarizes as silent, not as an error", () => {
		const { graph } = flowGraph([]);
		expect(trafficSummary(graph)).toMatchObject({ flows: 0, packets: 0, liveNodes: 0, newest: 0 });
	});
});

describe("freshness", () => {
	test("decays from 1 at the observation to 0 at the end of the fade", () => {
		const now = 1_000_000;
		expect(freshness(now, now)).toBe(1);
		expect(freshness(now - FRESHNESS_FADE_MS / 2, now)).toBeCloseTo(0.5);
		expect(freshness(now - FRESHNESS_FADE_MS, now)).toBe(0);
		expect(freshness(now - 10 * FRESHNESS_FADE_MS, now)).toBe(0);
	});

	test("a timestamp from the future clamps rather than exceeding 1", () => {
		expect(freshness(2000, 1000)).toBe(1);
	});
});

describe("packet animation", () => {
	test("busier flows carry more packets, up to a cap", () => {
		expect(packetCount(0)).toBe(MIN_PACKETS_PER_EDGE);
		expect(packetCount(1_000)).toBeGreaterThan(packetCount(10));
		expect(packetCount(100_000)).toBeGreaterThan(packetCount(1_000));
		expect(packetCount(10_000_000_000)).toBe(MAX_PACKETS_PER_EDGE);
	});

	// Log-scaled on purpose: real flow volumes span orders of magnitude, and a
	// linear mapping either saturates at the cap or leaves everything at one dot.
	test("a hundredfold more traffic is a few more packets, not a hundred", () => {
		expect(packetCount(100_000) - packetCount(1_000)).toBeLessThan(4);
	});

	test("busier flows move faster", () => {
		expect(transitMs(1_000_000)).toBeLessThan(transitMs(10));
	});

	test("packets stay on the edge and spread out along it", () => {
		const { graph } = flowGraph([
			{ key: flowKey, last_seen: 1, packets: 400, bytes: 1, status: "accepted" },
		]);
		const flow = observedFlows(graph)[0];
		const positions = packetPositions(flow, 1234);
		expect(positions.length).toBe(packetCount(400));
		for (const t of positions) {
			expect(t).toBeGreaterThanOrEqual(0);
			expect(t).toBeLessThan(1);
		}
		expect(new Set(positions).size).toBe(positions.length);
	});

	test("packets advance with time", () => {
		const { graph } = flowGraph([
			{ key: flowKey, last_seen: 1, packets: 10, bytes: 1, status: "accepted" },
		]);
		const flow = observedFlows(graph)[0];
		const t0 = packetPositions(flow, 0)[0];
		const t1 = packetPositions(flow, 100)[0];
		expect(t1).toBeGreaterThan(t0);
	});
});

describe("formatting", () => {
	test("bytes scale into binary units", () => {
		expect(formatBytes(512)).toBe("512 B");
		expect(formatBytes(16384)).toBe("16.0 KiB");
		expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MiB");
	});

	test("age reads in the largest unit that still says something", () => {
		expect(formatAge(4000)).toBe("4s ago");
		expect(formatAge(125_000)).toBe("2m ago");
		expect(formatAge(7_400_000)).toBe("2h ago");
	});
});
