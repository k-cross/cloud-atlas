import { describe, expect, test } from "bun:test";
import {
  DEFAULT_EDGE_COLOR,
  EDGE_COLORS,
  PROVIDER_COLORS,
  type Provider,
  nodeSize,
  providerOf,
} from "../src/style";

describe("providerOf", () => {
  // The kind strings are the Rust enum variant names from atlas-lib's
  // definition.rs; the prefix is the only thing that identifies the provider.
  test.each([
    ["AwsEc2Instance", "AWS"],
    ["AwsEni", "AWS"],
    ["GcpGkeCluster", "GCP"],
    ["GcpComputeInstance", "GCP"],
    ["AzureVirtualMachine", "Azure"],
    ["CloudflareZone", "Cloudflare"],
    ["ExternalService", "External"],
  ] as const)("%s -> %s", (kind, expected) => {
    expect(providerOf(kind)).toBe(expected);
  });

  test("unknown / generic kinds fall through to Generic", () => {
    expect(providerOf("GenericIpAddress")).toBe("Generic");
    expect(providerOf("GenericHostname")).toBe("Generic");
    expect(providerOf("SomethingBrandNew")).toBe("Generic");
    expect(providerOf("")).toBe("Generic");
  });

  test("prefix match is case-sensitive and anchored to the start", () => {
    // Guards against a loosened matcher (e.g. includes/toLowerCase) that would
    // mis-bucket kinds like a hypothetical 'MyAwsThing'.
    expect(providerOf("aws-lowercase")).toBe("Generic");
    expect(providerOf("MyAwsThing")).toBe("Generic");
  });

  test("every provider bucket has a color", () => {
    const providers: Provider[] = [
      "AWS",
      "GCP",
      "Azure",
      "Cloudflare",
      "External",
      "Generic",
    ];
    for (const p of providers) {
      expect(PROVIDER_COLORS[p]).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });
});

describe("EDGE_COLORS", () => {
  test("the cross-cloud seam edges are defined", () => {
    // RoutesTo (traffic) and ResolvesTo (DNS) are the cross-cloud stitching
    // edges; losing their styling would make the seams invisible.
    expect(EDGE_COLORS.RoutesTo).toBeDefined();
    expect(EDGE_COLORS.ResolvesTo).toBeDefined();
  });

  test("all edge colors are hex strings", () => {
    for (const color of Object.values(EDGE_COLORS)) {
      expect(color).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  test("DEFAULT_EDGE_COLOR is a hex string", () => {
    expect(DEFAULT_EDGE_COLOR).toMatch(/^#[0-9a-f]{6}$/i);
  });
});

describe("nodeSize", () => {
  test("isolated nodes get the base size", () => {
    expect(nodeSize(0)).toBe(2);
  });

  test("size grows with degree but sub-linearly (sqrt)", () => {
    expect(nodeSize(1)).toBe(4); // 2 + 2*sqrt(1)
    expect(nodeSize(4)).toBe(6); // 2 + 2*sqrt(4)
    expect(nodeSize(100)).toBe(22); // 2 + 2*sqrt(100)
  });

  test("is monotonic in degree", () => {
    let prev = -Infinity;
    for (let d = 0; d <= 50; d++) {
      const s = nodeSize(d);
      expect(s).toBeGreaterThanOrEqual(prev);
      prev = s;
    }
  });
});
