# Cross-Cloud Network Inference and Telemetry

## Overview
Cloud Atlas aims to be a continuous, live digital twin of multi-cloud environments. Currently, resources are mapped independently per cloud. This design outlines how we will:
1. Discover cross-cloud dependencies via IP overlap.
2. Infer external and unsupported services via security configurations.
3. Support future lightweight liveness and network telemetry via event-driven flow log ingestion.

## 1. Cross-Cloud Connections via Universal Pivot Nodes
We use the property graph model to organically merge disparate cloud environments by pivoting on universal identifiers.
- **Node Types**: `Node::GenericIpAddress` and `Node::GenericHostname` act as the common language between clouds.
- **Mapping**: For instance, Cloudflare DNS connects via `Edge::ResolvesTo -> Node::GenericIpAddress`, while an AWS EC2 Instance connects via `Edge::RoutesTo -> Node::GenericIpAddress`. 
- **Outcome**: The graph deduplicates identical nodes, seamlessly connecting Cloudflare and AWS without any direct API-level correlation.

## 2. Inferring External Services
For services without direct API integration, we infer their presence using security boundaries and DNS.
- **Security Boundaries**: We parse outbound rules in AWS Security Groups, GCP Firewalls, and Azure Network Security Groups, mapping explicit CIDRs to `GenericIpAddress` nodes via `Edge::RoutesTo`. Ranges wide enough to say nothing (`0.0.0.0/0` and friends) are dropped by `atlas::util::is_large_cidr` rather than pulling a meaningless pivot into the graph.
- **Cloud-Specific Abstractions**: Mappings that don't resolve to raw IPs (like Azure Service Tags) are given dedicated types (e.g. `Node::AzureServiceTag`) to prevent generic node pollution.
- **Graph paths, as built**: `AwsEc2SecurityGroup -> RoutesTo -> GenericIpAddress` for egress rules, and `CloudflareWorker -> ConnectsTo -> ExternalService` where a worker's secret/plain-text binding contains a URL. The originally sketched
  `SecurityGroup -> RoutesTo -> GenericIpAddress -> ResolvesTo -> ExternalService`
  chain is **not** built: nothing today resolves a raw CIDR to a named third-party service, so `Node::ExternalService` is only ever produced where a configuration literally names one.

## 3. Lightweight Network Telemetry
**Built** — `atlas-lib/src/atlas/flow.rs`, Tier 2 of `change_monitoring_design.md`.

- **Edge Definition**: `Edge::TrafficFlow` marks traffic that was *observed*
  between two endpoints. It carries no payload, which is a correction to this
  document's original sketch: `Edge` is the graph's identity type — hashed,
  deduplicated on insert, diffed by value — so storing `packet_count` on it
  would make every metric update a different edge, duplicating past
  `GraphBuilder::add_edge`'s dedup and emitting a remove-then-add of the same
  `edge_key` in every reconciliation patch. `status`, `packets`, `bytes` and
  `last_seen` therefore live in `flow::FlowIndex` beside the graph, keyed by the
  same stable `node_key`/`edge_key`. The same mechanism carries *node*
  freshness, which could never have been a field on `Node` for the same reason.
- **Ingestion**: No polling of the network itself. VPC Flow Logs deliver to S3,
  whose event notifications land on an SQS queue that `cloud/amazon/flow_logs.rs`
  drains (`--aws-flow-log-queue`). GCP's Log Router → Pub/Sub and Azure's VNet
  flow logs normalize into the same `FlowObservation`.
- **Cross-cloud, for free**: both endpoints are `GenericIpAddress` pivots, so a
  flow from an AWS instance to an address the Azure scan also reported becomes a
  real edge between the two estates — §1's merge, arrived at from the data plane
  instead of from DNS.
- **Performance**: edge updates are in-memory and the overlay is bounded and
  expiring, which is what decouples log ingestion volume from the size of the
  twin. An observation that lapses stops being folded into the scan graph, and
  the ordinary Tier-3 differ removes its edge — this tier never deletes anything
  itself.

### Known gap: observed traffic does not yet confirm §2's inferred reachability

A security-group rule projects `GenericIpAddress("198.51.100.0/24")`; a flow
record projects `GenericIpAddress("198.51.100.10")`. Generic-node identity is a
byte-exact `HashMap<Node, NodeIndex>` lookup, so the two never meet and "a flow
log proves the rule is actually used" does not work today. Closing it needs CIDR
*containment* — a prefix trie over the CIDR-shaped generic nodes, rebuilt per
scan — plus a decision on what edge kind owns the resulting link and which tier
is allowed to expire it. Tracked as an open question in
`docs/change_monitoring_design.md` §10, alongside the related problem that
generic-node values are never canonicalised (`2001:db8::1` and
`2001:0db8:0000:…` are two different pivots).
