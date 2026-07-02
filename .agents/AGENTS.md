# Cloud Atlas Project Guidelines

When working on Cloud Atlas, always adhere to the following architectural and design principles:

1. **Continuous Live Digital Twin:** The overarching goal of Cloud Atlas is to maintain a continuous, live, in-memory graph of the cloud environment synchronized via event streams (e.g., AWS EventBridge), *not* to be a static point-in-time snapshot generator. Keep long-running daemon execution in mind for future code.
2. **Property Graph Model:** The graph is modeled as a Property Graph using `petgraph::Graph<Node, Edge>`. Always use strongly typed custom `Node` and `Edge` enums in `atlas-lib/src/atlas/definition.rs` to represent semantic cloud relationships. Do not revert to raw string maps.
3. **Semantic Relationships:** When mapping new cloud resources, prioritize semantic networking paths. Map resources starting from the Elastic Network Interface (ENI) as the core pivot (e.g., `Instance -> HasIp -> ENI -> AttachedTo -> Subnet`).
4. **Output Format:** While the graph lives in memory, we support exporting to `.dot` files for visualization via Gephi. Ensure any new `Node` or `Edge` types implement `std::fmt::Display` to keep the visual output clean and labeled.
5. **Cross-Cloud Pivot Nodes:** Use `Node::GenericIpAddress` and `Node::GenericHostname` as standard integration points when resources communicate across clouds or external networks. Always use `Edge::RoutesTo` or `Edge::ResolvesTo` (for DNS) when connecting to these generic nodes to seamlessly stitch disconnected graph components.
