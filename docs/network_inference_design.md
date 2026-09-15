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

### Observed traffic confirms §2's inferred reachability

**Built** — `atlas-lib/src/atlas/containment.rs`.

A security-group rule projects `GenericIpAddress("203.0.113.0/24")`; a flow
record projects `GenericIpAddress("203.0.113.77")`. Generic-node identity is a
byte-exact `HashMap<Node, NodeIndex>` lookup, so the two never met and "a flow
log proves the rule is actually used" did not work. `containment::link` closes
it with containment rather than equality: once per pass it splits the graph's
generic pivots into ranges and addresses and links each address into every range
that covers it, giving the full chain

```
AwsEc2SecurityGroup -RoutesTo-> 203.0.113.0/24 -Covers-> 203.0.113.77 <-TrafficFlow- 10.10.1.11
```

- **`Edge::Covers` is its own kind.** It asserts something about a *rule*, not
  about traffic; reusing `RoutesTo` would make a derived link indistinguishable
  from one a scan reported.
- **Nothing owns it, because it is derived rather than observed.** It is a pure
  function of which pivots are present, so it is recomputed wherever a graph is
  finalized — after `carry_forward` and after `FlowIndex::overlay` — and never
  retained or expired on its own. Delete the rule and the range node goes with
  it; let the flow lapse and the address goes; either way the ordinary Tier-3
  differ removes the edge. `patch::carry_forward` correspondingly refuses to
  hold it (`Edge::is_projected`), since a derived edge is not a provider's
  evidence that anything still exists.
- **Matching is per-family and width-aware.** Addresses probe only the prefix
  widths the estate's rules actually wrote, so the pass costs what the rules
  cost, not the 33/129 widths a family could express. A CIDR written with host
  bits set (`198.51.100.10/24`) still matches on its network.
- **Range-to-range is not built.** One rule subsuming another is policy overlap,
  not traffic.

Both sides arrive here already canonicalised (`Node::ip`), so containment never
has to reason about spelling — see §10 of `docs/change_monitoring_design.md`.

## 4. Service Topology

**Designed, not built** — `docs/service_topology_design.md`.

§1-3 stop at the address. A flow record proves traffic moved between two
pivots; it does not yet say *which resources* were talking, because almost
nothing in the graph carries an address — only EC2 instances, and only their
primary private IP. An AWS load balancer's traffic rides ENIs that
`DescribeLoadBalancers` never returns, so `alb -> instance` traffic lands with
the balancer's side an orphan pivot. GCP instances drop `network_ip` entirely.

That document states the invariant this needs — *every resource that can appear
as a flow endpoint must carry its addresses in the graph* — and the derived
`Edge::Serves` that collapses
`A -> ip -TrafficFlow-> ip <- B` into one link between typed resources,
carrying an `inferred`/`confirmed` status beside the graph so a registered
target that receives nothing stays distinguishable from one that does not exist.
