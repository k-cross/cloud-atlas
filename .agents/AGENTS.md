# Cloud Atlas Project Guidelines

[`CLAUDE.md`](../CLAUDE.md) at the repo root is the authoritative guide. These eight invariants hold in every change:

1. **Continuous Live Digital Twin:** Maintain a live, in-memory graph synchronized via event streams, flow logs, and reconciliation scans—not a static snapshot generator. Write code for long-running daemons.
2. **Strongly Typed Property Graph:** Use `petgraph::Graph<Node, Edge>` with the enums in `atlas-lib/src/atlas/definition.rs`. Never use raw string maps.
3. **ENI as Networking Pivot:** Model networking through the interface (`Instance -HasIp-> ENI -AttachedTo-> Subnet`). Key `Node::AwsEc2Eni` by its `eni-` ID, never by an owning instance.
4. **Clean `.dot` Display:** Every `Node` and `Edge` implements `std::fmt::Display` in `Type::SubType(id)` form.
5. **Cross-Cloud & Scoped Pivots:** Construct `GenericIpAddress`, `GenericPrivateIpAddress`, and `GenericHostname` exclusively via `Node::ip` / `Node::public_ip` / `Node::hostname`. Private IPs are scoped to their network (`{ network, address }`). Edge kinds are load-bearing: `ConnectsTo` = holds address, `RoutesTo` = traffic/rules, `ResolvesTo` = DNS (`GenericHostname -ResolvesTo-> <record> -ResolvesTo-> target`). Every projected edge must have at least one owned endpoint.
6. **Failure ≠ Absence:** Collectors report failures on `CollectionReport` (`atlas::collection`) rather than returning empty sets; never swallow errors with `.ok()`. Unreadable sources are carried forward for a bounded number of scans.
7. **Identity Only on `Node` and `Edge`:** `Hash + Eq` is identity for dedup, diffing, and wire keys. Mutable counters, `last_seen`, and status live beside the graph (`FlowIndex` and derived observations), keeping `Edge::TrafficFlow` and `Edge::Serves` payload-free.
8. **Distinct Tier Rights:** Tier 1 (events) and Tier 2 (flow logs) only add (except explicit Tier-1 node deletes); only Tier 3 (full-scan reconciliation) garbage-collects. Event adapters may only emit shapes that the full-scan projector also emits.

## Testing

No live cloud credentials exist locally. Projectors are tested against the "Globex" fixtures (`atlas-lib/src/fixtures.rs`); collectors are tested with canned HTTP/SDK responses (`wiremock` for reqwest, `StaticReplayClient` for AWS SDK). `cargo xtask test` runs all suites in order.
