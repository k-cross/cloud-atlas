export type Provider = "AWS" | "GCP" | "Azure" | "Cloudflare" | "External" | "Generic";

export function providerOf(kind: string): Provider {
	if (kind.startsWith("Aws")) return "AWS";
	if (kind.startsWith("Gcp")) return "GCP";
	if (kind.startsWith("Azure")) return "Azure";
	if (kind.startsWith("Cloudflare")) return "Cloudflare";
	if (kind === "ExternalService") return "External";
	return "Generic";
}

export const PROVIDER_COLORS: Record<Provider, string> = {
	AWS: "#ff9900",
	GCP: "#4285f4",
	Azure: "#00b7c3",
	Cloudflare: "#f4442e",
	External: "#b07cd8",
	Generic: "#8b93a1",
};

export const DEFAULT_EDGE_COLOR = "#333b47";

export const EDGE_COLORS: Record<string, string> = {
	Contains: "#333b47",
	AttachedTo: "#333b47",
	ConnectsTo: "#54604f",
	DependsOn: "#5c4f36",
	HasIp: "#38585e",
	RoutesTo: "#2f6285",
	ResolvesTo: "#6c4a78",

	TrafficFlow: "#3f8f6f",
	Covers: "#3d4f60",
};

export const FLOW_STATUS_COLORS: Record<string, string> = {
	accepted: "#3fbf87",
	rejected: "#e0576a",
	mixed: "#e0a83f",
	observed: "#7f8ea3",
};

export const FLOW_PACKET_COLORS: Record<string, string> = {
	accepted: "#d6ffee",
	rejected: "#ffd8de",
	mixed: "#ffeccb",
	observed: "#e3ebf6",
};

export function packetColor(status: string | undefined): string {
	return (status && FLOW_PACKET_COLORS[status]) || FLOW_PACKET_COLORS.observed;
}

export function flowColor(status: string | undefined): string {
	return (status && FLOW_STATUS_COLORS[status]) || EDGE_COLORS.TrafficFlow;
}

export function flowSize(packets: number | undefined): number {
	return 1 + Math.log10((packets ?? 0) + 1);
}

export function nodeSize(degree: number): number {
	return 2 + 2 * Math.sqrt(degree);
}
