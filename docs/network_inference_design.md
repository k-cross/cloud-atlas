# Cross-Cloud Network Inference and Telemetry

How separately scanned clouds are joined into one graph, and how observed traffic is laid over it.

## 1. Universal pivot nodes

`GenericIpAddress` and `GenericHostname` are the shared vocabulary between clouds. Producers build them only through `Node::ip`/`Node::hostname`, which canonicalise spelling (`util::canonical_address`: `IpAddr` round-trip, `::ffff:` unmapping, CIDR re-basing; `util::canonical_hostname`: lowercase, no trailing dot). Unparseable values pass through untouched. Because `GraphBuilder` dedups by value, an AWS interface that `ConnectsTo` an address and a Cloudflare record that `ResolvesTo` it meet at the same node with no API-level correlation.

**Only public addresses are shared vocabulary.** A private value — RFC 1918, RFC 6598 shared space, link-local, loopback, ULA (`util::is_private`) — names a different host in every network that uses it, and default VPCs number themselves from `172.31.0.0/16` in every account and region. So `Node::ip(value, network)` builds `GenericPrivateIpAddress { network, address }` for a private value and ignores `network` for a public one. `network` is where the address was read: the VPC, VNet or GCP network of the resource that holds or names it, or of the interface that captured a flow record. Where nothing names a network — a DNS record with a private answer, an NSG attached to no subnet — the naming resource stands in, so the address is unique to it and joins nothing. A CIDR is private only if it lies wholly inside one block; rules narrower than `/16` (the only ones `is_large_cidr` keeps) never straddle one.

Traffic between networks over peering, a transit gateway, a VPN or an interconnect carries the far side's private address, read in the near side's network. Joins that are modelled resolve it: an address nothing holds where it was read takes the holder in the one joined network that has it (`network_connectivity_design.md`; phase 1 covers peering and transit gateways). Until VPNs and interconnects are modelled (phase 2), the fixture's AWS flows to an Azure NIC and a Cloud SQL instance land on addresses nobody in that network holds — the honest answer, not a match on value. A VPC numbered from public space shares those pivots with the internet, as it shares the addresses.

DNS always takes one shape, `GenericHostname(name) -ResolvesTo-> record -ResolvesTo-> target`, so a CNAME in one provider chains through a name another provider defines. The record sits between the two pivots because an edge joining two ownerless nodes belongs to no provider, and `carry_forward` would keep it through any provider's outage. The fixtures spell three seams differently on each side so the guard tests catch a producer that bypasses the constructors.

## 2. Inferring external services

- **Security rules.** Explicit CIDRs in AWS security groups, GCP firewalls and Azure NSGs become `GenericIpAddress` via `RoutesTo`. Ranges too wide to mean anything (`0.0.0.0/0`, …) are dropped by `util::is_large_cidr`.
- **Non-IP targets** (e.g. Azure service tags) get dedicated kinds such as `AzureServiceTag`.
- **Named services.** `CloudflareWorker -ConnectsTo-> ExternalService` where a secret or plain-text binding contains a URL. Nothing maps a raw CIDR to a named third party, so `ExternalService` appears only where configuration names one.

## 3. Flow telemetry (Tier 2)

Implemented in `atlas/flow.rs`; see `change_monitoring_design.md` §7.

- `Edge::TrafficFlow` means only "traffic was observed". Status, packets, bytes and `last_seen` live in `FlowIndex`, keyed by `node_key`/`edge_key`, because `Edge` is an identity type; node freshness uses the same mechanism.
- Ingestion is push-based: VPC Flow Logs → S3 → SQS (`--aws-flow-log-queue`). GCP and Azure flow logs will normalize into the same `FlowObservation`.
- Both endpoints are pivots, so a flow between two clouds becomes a real edge between their estates. A private endpoint is read in the network that captured the record — the record's `vpc-id`, else its interface's network in the live graph — and a record that offers neither is dropped and reported rather than placed by guess.
- The overlay is bounded and expiring; a lapsed flow is dropped from the next scan graph and the differ removes its edge.

## 4. Containment: traffic confirms a rule

Implemented in `atlas/derive/containment.rs`.

A rule yields `GenericIpAddress("203.0.113.0/24")` and a flow yields `GenericIpAddress("203.0.113.77")`; exact-value identity never joins them. The containment pass links each address into every range covering it:

```
AwsEc2SecurityGroup -RoutesTo-> 203.0.113.0/24 -Covers-> 203.0.113.77 <-TrafficFlow- 10.10.1.11
```

- **`Covers` is its own kind** — it describes a rule, not traffic, and must not be confused with a scanned `RoutesTo`.
- **Derived, so owned by no tier.** Recomputed at every finalisation point via `derive::all`; lifecycle comes from the differ; `carry_forward` never holds it (`Edge::is_projected`).
- **Per-family and width-aware.** Addresses probe only the prefix widths the estate's rules use. A CIDR with host bits set still matches on its network.
- **Per-network.** A private range covers private addresses read in its own network only; a public range covers public addresses.
- **No range-to-range edges** — rule overlap is policy, not traffic.

## 5. Service topology

A flow proves traffic between addresses, not between resources. `service_topology_design.md` covers giving every flow endpoint an address in the graph and the derived `Edge::Serves` that collapses the resulting path into one link between typed resources.
