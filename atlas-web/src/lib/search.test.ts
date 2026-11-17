import { describe, expect, test } from "bun:test";
import Graph from "graphology";
import { excerpt, searchNodes, searchTokens } from "./search";

function estate(): Graph {
	const graph = new Graph({ multi: true, type: "directed" });
	const node = (key: string, label: string, kind: string) =>
		graph.addNode(key, { label, kind, x: 0, y: 0 });
	node("web-01", "AWS::Ec2Instance(i-globex-web-01)", "AwsEc2Instance");
	node("web-02", "AWS::Ec2Instance(i-globex-web-02)", "AwsEc2Instance");
	node("eni", "AWS::Ec2Eni(eni-globex-web-01a)", "AwsEc2Eni");
	node("ip", "Generic::PrivateIpAddress(10.10.1.10@vpc-globex)", "GenericPrivateIpAddress");
	node("router", "AWS::EC2::VPC(vpc-globex)", "AwsEc2Vpc");
	node(
		"vnet",
		"Azure::Network::VirtualNetwork(/subscriptions/sub-globex/resourcegroups/rg-globex/providers/microsoft.network/virtualnetworks/vnet-globex)",
		"AzureVirtualNetwork",
	);
	return graph;
}

describe("node search", () => {
	test("an empty query matches nothing and dims nothing", () => {
		const result = searchNodes(estate(), "   ");
		expect(result.hits).toEqual([]);
		expect(result.matches.size).toBe(0);
	});

	test("matching is case-insensitive and every token must match", () => {
		const result = searchNodes(estate(), "WEB 01");
		expect([...result.matches].sort()).toEqual(["eni", "web-01"]);
	});

	test("a whole-word match outranks a prefix of a longer word", () => {
		const result = searchNodes(estate(), "web-01");
		expect(result.hits.map((h) => h.key)).toEqual(["web-01", "eni"]);
	});

	test("the kind is searched when the label does not carry the token", () => {
		const result = searchNodes(estate(), "ec2instance");
		expect(result.total).toBe(2);
		expect(result.hits.every((h) => h.kind === "AwsEc2Instance")).toBe(true);
	});

	test("addresses are found by any prefix of their dotted form", () => {
		expect(searchNodes(estate(), "10.10.1").hits.map((h) => h.key)).toEqual(["ip"]);
	});

	test("the hit list is capped but the total and the dimming set are not", () => {
		const result = searchNodes(estate(), "globex", () => true, 2);
		expect(result.hits).toHaveLength(2);
		expect(result.total).toBe(6);
		expect(result.matches.size).toBe(6);
	});

	test("a node the current view hides is not offered", () => {
		const result = searchNodes(estate(), "globex", (key) => key !== "vnet");
		expect(result.matches.has("vnet")).toBe(false);
	});
});

describe("search excerpts", () => {
	const text = (label: string, query: string, width?: number) =>
		excerpt(label, searchTokens(query), width)
			.map((s) => (s.match ? `[${s.text}]` : s.text))
			.join("");

	test("a short label is shown whole with its matches marked", () => {
		expect(text("AWS::Ec2Instance(i-globex-web-01)", "web")).toBe(
			"AWS::Ec2Instance(i-globex-[web]-01)",
		);
	});

	test("a long label keeps the window around its first match", () => {
		const label =
			"Azure::Network::VirtualNetwork(/subscriptions/sub-globex/resourcegroups/rg-globex/providers/microsoft.network/virtualnetworks/vnet-globex)";
		const shown = text(label, "vnet-globex", 30);
		expect(shown).toContain("[vnet-globex]");
		expect(shown.startsWith("…")).toBe(true);
	});

	test("a long label matched only by kind shows its tail, where the name is", () => {
		const label = `Azure::Network::VirtualNetwork(${"x".repeat(60)}/vnet-globex)`;
		expect(text(label, "azurevirtualnetwork", 20).endsWith("vnet-globex)")).toBe(true);
	});

	test("overlapping tokens are marked once", () => {
		expect(text("i-globex-web-01", "glob globex")).toBe("i-[globex]-web-01");
	});
});
