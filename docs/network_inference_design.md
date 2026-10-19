# Cross-Cloud Network Inference & Telemetry

How separately scanned clouds join into one graph and how observed traffic overlays it.

## 1. Universal Pivot Nodes

`GenericIpAddress`, `GenericPrivateIpAddress`, and `GenericHostname` are the shared vocabulary across clouds, built exclusively via `Node::ip` / `Node::public_ip` / `Node::hostname`:

- **Canonicalisation:** `util::canonical_address` round-trips `IpAddr`, unmaps `::ffff:`, and zeroes host bits on CIDRs; `util::canonical_hostname` lowercases and strips trailing dots. Unparseable values pass through untouched. `GraphBuilder` deduplicates by value, so an AWS ENI (`ConnectsTo`) and a Cloudflare DNS record (`ResolvesTo`) naming the same public IP meet at one node without API correlation.
- **Public vs. private scope:** Only public addresses join across networks. Private ranges—RFC 1918, RFC 6598, link-local, loopback, and ULA (`util::is_private`)—build `GenericPrivateIpAddress { network, address }`, where `network` is the VPC, VNet, or GCP network in which the address was read (or the capturing interface's network for flow logs). When no network exists (e.g. a DNS record with a private answer or an unattached NSG), the naming resource's own ID stands in as `network` so it joins nothing. A CIDR is private only if it lies wholly within a private block; CIDRs wider than `/16` are dropped by `util::is_large_cidr`.
- **Cross-network private traffic:** Traffic over peering, transit gateways, VPNs, or interconnects carries the remote private IP read in the local network. Modelled joins resolve it at derive time (`network_connectivity_design.md`); unmodelled hops (currently VPN/interconnect flows to Azure and Cloud SQL in the fixtures) remain on unheld local-scope pivots until Phase 2 lands. Publicly numbered VPCs share `GenericIpAddress` pivots with the internet.
- **Uniform DNS shape:** Every DNS provider projects `GenericHostname(name) -ResolvesTo-> <record> -ResolvesTo-> target`. CNAME chains stitch across providers, and the owned record node prevents ownerless-to-ownerless edges that `carry_forward` could never expire. Fixture seams deliberately vary casing and formatting on each side so tests catch constructors bypassed by hand.

## 2. External Services & Security Rules

- **CIDR rules:** Explicit CIDRs in AWS Security Groups, GCP Firewalls, and Azure NSGs link via `RoutesTo -> GenericIpAddress` / `GenericPrivateIpAddress` (filtered by `util::is_large_cidr` to exclude `0.0.0.0/0` and overly broad blocks).
- **Non-IP targets:** Provider-specific constructs use dedicated kinds (e.g. `AzureServiceTag`).
- **Named external services:** `CloudflareWorker -ConnectsTo-> ExternalService` is emitted only when a binding or secret explicitly names a URL; raw CIDRs are never heuristically mapped to third-party vendors.

## 3. Flow Telemetry (Tier 2)

Implemented in `atlas/flow.rs`:

- `Edge::TrafficFlow` asserts only that traffic occurred. Verdict, packet/byte counters, and `last_seen` live beside the graph in the bounded, expiring `FlowIndex` keyed by `node_key`/`edge_key`.
- Both endpoints are IP pivots, turning cross-cloud traffic into edges between estates. Private endpoints resolve in the record's `vpc-id` or its `interface-id`'s network in the live graph; records with a private IP and neither are dropped and reported as `Malformed`.
- Expired flows are omitted when `FlowIndex::overlay` folds into the next Tier-3 scan, and `patch::diff` removes their edges.

## 4. Containment (`Edge::Covers`)

Implemented in `atlas/derive/containment.rs`. Security rules name CIDR blocks (`203.0.113.0/24`) while flow logs name host IPs (`203.0.113.77`). The containment pass links each host pivot to every CIDR pivot covering it:

```
AwsEc2SecurityGroup -RoutesTo-> 203.0.113.0/24 -Covers-> 203.0.113.77 <-TrafficFlow- 10.10.1.11
```

- **Distinct edge kind:** `Covers` represents CIDR inclusion, never traffic (`RoutesTo`).
- **Stateless derivation:** Recomputed by `derive::all` at every finalisation point, diffed like any edge, and excluded from `carry_forward` (`!Edge::is_projected()`).
- **Scoped & width-probed:** Host addresses probe only prefix lengths present in the estate's rules, within the same IP family and network scope (private ranges cover only private addresses in the same `network`; no range-to-range edges).

## 5. Service Topology

Flow logs connect addresses, not resources. [`docs/service_topology_design.md`](service_topology_design.md) covers resource address projection and the derived `Edge::Serves` that collapses multi-hop paths into direct resource-to-resource links.
