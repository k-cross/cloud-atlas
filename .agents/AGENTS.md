# Cloud Atlas Project Guidelines

`CLAUDE.md` at the repo root is the full guide (architecture rules, testing
patterns, established helpers, dev orchestration). This file is the short form —
the principles that must hold in every change. When the two disagree, `CLAUDE.md`
is authoritative and this file is out of date.

1. **Continuous Live Digital Twin:** The overarching goal of Cloud Atlas is to maintain a continuous, live, in-memory graph of the cloud environment synchronized via event streams (e.g., AWS EventBridge), *not* to be a static point-in-time snapshot generator. Keep long-running daemon execution in mind for future code.
2. **Property Graph Model:** The graph is modeled as a Property Graph using `petgraph::Graph<Node, Edge>`. Always use strongly typed custom `Node` and `Edge` enums in `atlas-lib/src/atlas/definition.rs` to represent semantic cloud relationships. Do not revert to raw string maps.
3. **Semantic Relationships:** When mapping new cloud resources, prioritize semantic networking paths. Map resources starting from the Elastic Network Interface (ENI) as the core pivot (e.g., `Instance -> HasIp -> ENI -> AttachedTo -> Subnet`). `Node::AwsEc2Eni` is keyed by the interface's own `eni-` id, never by its owning instance — an ENI can be reattached, and most ENIs belong to no instance at all.
4. **Output Format:** While the graph lives in memory, we support exporting to `.dot` files for visualization via Gephi. Ensure any new `Node` or `Edge` types implement `std::fmt::Display` to keep the visual output clean and labeled.
5. **Cross-Cloud Pivot Nodes:** Use `Node::GenericIpAddress` and `Node::GenericHostname` as standard integration points when resources communicate across clouds or external networks. Always use `Edge::RoutesTo` or `Edge::ResolvesTo` (for DNS) when connecting to these generic nodes to seamlessly stitch disconnected graph components.
6. **A failure must never look like an absence.** This is a live graph, so "we could not read it" and "it is gone" have to stay distinguishable all the way to the differ. Collectors report what they could not read on a `CollectionReport` (`atlas::collection`) rather than returning an empty collection; never swallow an error with `.ok()`. An unreadable source has its resources carried forward instead of deleted, for a bounded number of scans.
7. **`Node` and `Edge` carry identity, never mutable state.** Both are `Hash + Eq` and that *is* their identity — the builder dedups on it, the differ compares on it, and the wire key derives from it. A field that changes while the resource stays the same (a packet counter, a `last_seen`) would make every update a different value. Such properties go beside the graph, keyed by the stable key; `atlas::flow::FlowIndex` is the one instance, and the reason `Edge::TrafficFlow` is payload-free.
8. **Tiers have distinct rights.** Tier 1 (event streams) and Tier 2 (flow logs) only ever *add*; only Tier 3 (the reconciliation scan) garbage-collects. An adapter may only produce nodes and edges the full-scan projector would also produce, or the two tiers undo each other forever.

## Testing

No live cloud credentials are available locally. Projectors are tested against the
fake "Globex" environment in `atlas-lib/src/fixtures.rs`; collectors are tested by
replaying canned HTTP/SDK responses (wiremock for the reqwest clients,
`StaticReplayClient` for the AWS SDK). Never write a test that needs a real cloud
API call. `cargo xtask test` runs every suite in the repo in order.
