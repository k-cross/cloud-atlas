import { describe, expect, test } from "bun:test";
import { DEFAULT_EDGE_COLOR, EDGE_COLORS, nodeSize, PROVIDER_COLORS, providerOf } from "./style";

describe("providerOf", () => {
	test("buckets kinds by their Rust enum-variant prefix", () => {
		expect(providerOf("AwsEc2Instance")).toBe("AWS");
		expect(providerOf("GcpComputeInstance")).toBe("GCP");
		expect(providerOf("AzureVirtualMachine")).toBe("Azure");
		expect(providerOf("CloudflareWorker")).toBe("Cloudflare");
		expect(providerOf("ExternalService")).toBe("External");
		expect(providerOf("GenericIpAddress")).toBe("Generic");
		expect(providerOf("GenericHostname")).toBe("Generic");
	});

	test("every provider bucket has a color", () => {
		for (const kind of ["AwsX", "GcpX", "AzureX", "CloudflareX", "ExternalService", "GenericX"]) {
			expect(PROVIDER_COLORS[providerOf(kind)]).toMatch(/^#[0-9a-f]{6}$/i);
		}
	});
});

describe("EDGE_COLORS", () => {
	test("known edge kinds have a color, unknown ones fall back", () => {
		expect(EDGE_COLORS.RoutesTo).toBe("#2f6285");
		expect(EDGE_COLORS.ResolvesTo).toBe("#6c4a78");
		expect(EDGE_COLORS.SomeFutureEdge ?? DEFAULT_EDGE_COLOR).toBe(DEFAULT_EDGE_COLOR);
	});
});

describe("nodeSize", () => {
	test("is degree-scaled (sqrt) with a non-zero base for leaves", () => {
		expect(nodeSize(0)).toBe(2);
		expect(nodeSize(1)).toBe(4);
		expect(nodeSize(4)).toBe(6);
		// Monotonic but sub-linear so hubs stand out without swamping the view.
		expect(nodeSize(100)).toBeLessThan(nodeSize(0) + 100);
		expect(nodeSize(9)).toBeGreaterThan(nodeSize(4));
	});
});
