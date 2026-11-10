# Checkpoint

## Current State

All four providers collect, all three change-detection tiers run for AWS, and the full stack runs credential-free in demo mode.

- **`atlas-lib`:** Collectors and projectors for AWS, GCP, Azure, and Cloudflare; 99 `Node` kinds and 10 `Edge` kinds enforced by exhaustiveness guards.
- **Tier 1 (Events):** `atlas::event` + `cloud/amazon/events.rs` (AWS Config items including ENIs, EC2 state changes, and CloudTrail over SQS).
- **Tier 2 (Flows):** `atlas::flow` + `cloud/amazon/flow_logs.rs` (VPC Flow Logs → S3 → SQS into `FlowIndex`).
- **Tier 3 (Reconciliation):** `patch::diff` over a persistent graph; `CollectionReport` + `carry_forward` + `Retention` distinguish collection failures from deletions.
- **Address & network coverage:** AWS `DescribeNetworkInterfaces` and RDS endpoint hostnames; GCP instance internal/NAT IPs, forwarding rules, and Cloud SQL; Azure NICs and load balancers. Private IPs scope to their network (`GenericPrivateIpAddress`) and resolve across AWS VPC peering, Transit Gateways and their peering, Azure VNet peering and Virtual WAN, GCP network peering and NCC hubs, site-to-site VPN tunnels, and dedicated circuits (Direct Connect, ExpressRoute and Cloud Interconnect, paired across clouds over Cross-Cloud Interconnect), with each network's address space keeping its own unheld addresses local and VPC and transit gateway route tables deciding which join traffic takes (`docs/network_connectivity_design.md`).
- **Derived edges (`atlas::derive::all`):** `Covers` (host IP in CIDR, per network) and `Serves` (`inferred`/`confirmed` service topology across instance and IP target groups, gated on `scanned_instances`).
- **`atlas-server` & `atlas-web`:** `/snapshot.json` (v3), `/collection.json` (with per-source retention), `/ws` (pushes `collection` health beside snapshots and patches); Wasm ForceAtlas2 layout + Sigma.js WebGL with traffic overlay, `Serves` status styling, service view, node search with fly-to focus, and a Collection health panel.
- **Demo:** A scripted 16-tick loop (`atlas-server/src/demo.rs`) launches web-04 behind the balancer, confirms it with traffic, quiets web-01, holds Azure through a three-scan outage, then recovers (`docs/demo_design.md`).
- **`cargo xtask`:** `dev [--demo]`, `wasm`, `demo`, `test [--e2e]`.

## Next Up

1. **Tiers 1–2 for GCP and Azure** — Cloud Asset Inventory → Pub/Sub, Azure Event Grid, and GCP/Azure flow logs into `FlowObservation` (`docs/change_monitoring_design.md`). Azure flow logs would also let the demo show the ExpressRoute circuit carrying traffic.
2. **Rendering Phase 3 remainder** — tooltips, drill-down, and raw observation values (`docs/graph_rendering_design.md`); search (Phase 4) is done.
3. **Network connectivity Phase 4** — Azure user-defined routes and GCP routes, Virtual WAN routing, NCC hybrid spokes, link-to-link transit (`docs/network_connectivity_design.md`); Phases 1–3 (peering, transit gateways, VPN, dedicated circuits, address spaces, route tables, hubs) are done.

## Backlog

- **Per-node provenance:** Required for cross-tier ordering (preventing a scan from overwriting a newer mid-scan event) and finer-grained outage retention (currently per provider).
- **WebSocket backpressure:** Per-client patch coalescing under high churn.
- **`cargo xtask dev` flag forwarding:** Forward `--aws-event-queue`, `--aws-flow-log-queue`, `--retain-scans`, and `--flow-ttl-secs`.
- **GCP and Azure `CHAINS` rows:** Add GCP backend-service and Azure load-balancer backend-pool kinds for `inferred` `Serves` edges.
- **Route 53 private zones & alias records:** Scope private answers to the zone's associated VPCs (`GetHostedZone`) rather than the zone itself, so private DNS reaches the ENIs it names; project `AliasTarget` (balancers, CloudFront, S3 websites), which today resolves to nothing.
- **NIC-only Azure NSGs:** Collect NIC `networkSecurityGroup` references so NSGs not attached to a subnet inherit the NIC's VNet scope.
- **Address reuse inherits confirmation:** `confirmed` is keyed by address pair, so a resource that takes an address within the flow TTL of its previous holder's traffic (15 min live) is `confirmed` by flows that predate it. Fixing it needs per-node first-seen times (see per-node provenance). Found by the demo loop; the demo avoids it by outlasting the TTL before relaunching web-04.
- **Stopped-instance ENIs:** A stopped instance's ENI keeps its IP in the graph, so traffic still hitting that IP derives `Serves` to `AwsEc2Eni` (`docs/service_topology_design.md` §1).
- **EventBridge buses:** `eventbridge` collects buses every scan but the projector only logs them. Draw them as nodes or drop the collector.
- **Demo follow-ups (F3, F4, F6):** Isolated guard nodes, short labels, Tier 1 in demo mode (`docs/demo_design.md`).

## Resume

```bash
cargo xtask test [--e2e]
cargo xtask dev --demo
cargo run --example demo
```

Context: [`CLAUDE.md`](../CLAUDE.md), [`docs/change_monitoring_design.md`](change_monitoring_design.md), [`docs/service_topology_design.md`](service_topology_design.md), [`docs/network_connectivity_design.md`](network_connectivity_design.md), [`docs/demo_design.md`](demo_design.md), [`docs/audit_findings.md`](audit_findings.md).
