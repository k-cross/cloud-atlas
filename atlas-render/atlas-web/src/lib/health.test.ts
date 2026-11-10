import { describe, expect, test } from "bun:test";
import { type CollectionHealth, healthLevel, healthRows } from "./health";

function health(overrides: Partial<CollectionHealth> = {}): CollectionHealth {
	return {
		complete: true,
		unreadable: [],
		retention: [],
		failures: [],
		stream: { healthy: true, failures: [] },
		flows: { healthy: true, failures: [], observed: 0 },
		...overrides,
	};
}

const outage = {
	source: "azure",
	kind: "unavailable",
	scope: "resource_graph",
	message: "503 Service Unavailable",
};

describe("collection health", () => {
	test("a complete scan with healthy feeds has nothing to report", () => {
		const rows = healthRows(health());
		expect(rows).toEqual([]);
		expect(healthLevel(rows)).toBe("ok");
	});

	test("a held source says how far into its budget it is", () => {
		const rows = healthRows(
			health({
				complete: false,
				unreadable: ["Azure"],
				retention: [{ source: "azure", kind: "unavailable", scans: 2, budget: 10, held: true }],
				failures: [outage],
			}),
		);
		expect(rows).toEqual([
			{
				subject: "Azure",
				detail: "unavailable · holding 2/10 scans",
				level: "holding",
				failures: ["resource_graph: 503 Service Unavailable"],
			},
		]);
		expect(healthLevel(rows)).toBe("holding");
	});

	test("a source past its budget reads as released, the worst state", () => {
		const rows = healthRows(
			health({
				complete: false,
				retention: [{ source: "azure", kind: "unavailable", scans: 11, budget: 10, held: false }],
				failures: [outage],
				stream: { healthy: false, failures: [{ ...outage, source: "aws", scope: "events" }] },
			}),
		);
		expect(rows.map((r) => [r.subject, r.level])).toEqual([
			["Azure", "released"],
			["Event feed", "degraded"],
		]);
		expect(healthLevel(rows)).toBe("released");
	});

	test("malformed rows degrade a source without holding it", () => {
		const rows = healthRows(
			health({
				complete: false,
				failures: [
					{ source: "azure", kind: "malformed", scope: "row a", message: "unknown variant" },
					{ source: "azure", kind: "malformed", scope: "row b", message: "unknown variant" },
				],
			}),
		);
		expect(rows).toHaveLength(1);
		expect(rows[0].detail).toBe("2 malformed rows · removals still apply");
		expect(rows[0].level).toBe("degraded");
	});

	test("an unreadable source with no retention yet is still reported", () => {
		const rows = healthRows(
			health({
				complete: false,
				failures: [{ ...outage, source: "gcp", kind: "unauthorized", scope: "credentials" }],
			}),
		);
		expect(rows[0]).toMatchObject({
			subject: "GCP",
			detail: "unauthorized · nothing held yet",
			level: "released",
		});
	});

	test("a degraded flow feed is its own row", () => {
		const rows = healthRows(
			health({
				flows: {
					healthy: false,
					failures: [{ ...outage, source: "aws", scope: "flow-logs" }],
					observed: 3,
				},
			}),
		);
		expect(rows).toEqual([
			{
				subject: "Flow feed",
				detail: "1 failure",
				level: "degraded",
				failures: ["flow-logs: 503 Service Unavailable"],
			},
		]);
	});
});
