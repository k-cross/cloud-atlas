# Cross-Cloud Network Inference and Telemetry

How separately scanned clouds are joined into one graph, and how observed traffic is laid over it.

## 1. Universal pivot nodes

`GenericIpAddress` and `GenericHostname` are the shared vocabulary between clouds. Producers build them only through `Node::ip`/`Node::hostname`, which canonicalise spelling (`util::canonical_address`: `IpAddr` round-trip, `::ffff:` unmapping, CIDR re-basing; `util::canonical_hostname`: lowercase, no trailing dot). Unparseable values pass through untouched. Because `GraphBuilder` dedups by value, an AWS interface that `ConnectsTo` an address and a Cloudflare record that `ResolvesTo` it meet at the same node with no API-level correlation. The fixtures spell three seams differently on each side so the guard tests catch a producer that bypasses the constructors.

## 2. Inferring external services

- **Security rules.** Explicit CIDRs in AWS security groups, GCP firewalls and Azure NSGs become `GenericIpAddress` via `RoutesTo`. Ranges too wide to mean anything (`0.0.0.0/0`, …) are dropped by `util::is_large_cidr`.
- **Non-IP targets** (e.g. Azure service tags) get dedicated kinds such as `AzureServiceTag`.
- **Named services.** `CloudflareWorker -ConnectsTo-> ExternalService` where a secret or plain-text binding contains a URL. Nothing maps a raw CIDR to a named third party, so `ExternalService` appears only where configuration names one.

## 3. Flow telemetry (Tier 2)

Implemented in `atlas/flow.rs`; see `change_monitoring_design.md` §7.

- `Edge::TrafficFlow` means only "traffic was observed". Status, packets, bytes and `last_seen` live in `FlowIndex`, keyed by `node_key`/`edge_key`, because `Edge` is an identity type; node freshness uses the same mechanism.
- Ingestion is push-based: VPC Flow Logs → S3 → SQS (`--aws-flow-log-queue`). GCP and Azure flow logs will normalize into the same `FlowObservation`.
- Both endpoints are pivots, so a flow between two clouds becomes a real edge between their estates.
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
- **No range-to-range edges** — rule overlap is policy, not traffic.

## 5. Service topology

A flow proves traffic between addresses, not between resources. `service_topology_design.md` covers giving every flow endpoint an address in the graph and the derived `Edge::Serves` that collapses the resulting path into one link between typed resources.
