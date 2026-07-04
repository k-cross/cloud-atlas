// Styling is keyed off the snapshot `kind` strings — the Rust enum variant
// names from atlas-lib's `definition.rs` (`AwsEc2Instance`, `GcpGkeCluster`,
// `Edge::RoutesTo`, ...). The prefix identifies the provider.

export type Provider =
  | "AWS"
  | "GCP"
  | "Azure"
  | "Cloudflare"
  | "External"
  | "Generic";

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

// Muted so structure reads at a glance without shouting over the nodes;
// traffic/DNS edges (the cross-cloud seams) get the most saturation.
export const EDGE_COLORS: Record<string, string> = {
  Contains: "#333b47",
  AttachedTo: "#333b47",
  ConnectsTo: "#54604f",
  DependsOn: "#5c4f36",
  HasIp: "#38585e",
  RoutesTo: "#2f6285",
  ResolvesTo: "#6c4a78",
};

// Degree-scaled size: hubs (VPCs, subnets, zones) stand out, leaves stay
// small enough that ten-thousand-node graphs don't turn into a solid disc.
export function nodeSize(degree: number): number {
  return 2 + 2 * Math.sqrt(degree);
}
