# Checkpoint

## Current State

All four providers collect, all three change-detection tiers run for AWS, and the full stack runs credential-free in demo mode.

- **`atlas-lib`:** Collectors and projectors for AWS, GCP, Azure, and Cloudflare; 74 `Node` kinds and 10 `Edge` kinds enforced by exhaustiveness guards.
- **Tier 1 (Events):** `atlas::event` + `cloud/amazon/events.rs` (AWS Config items including ENIs, EC2 state changes, and CloudTrail over SQS).
- **Tier 2 (Flows):** `atlas::flow` + `cloud/amazon/flow_logs.rs` (VPC Flow Logs → S3 → SQS into `FlowIndex`).
- **Tier 3 (Reconciliation):** `patch::diff` over a persistent graph; `CollectionReport` + `carry_forward` + `Retention` distinguish collection failures from deletions.
- **Address & network coverage:** AWS `DescribeNetworkInterfaces` and RDS endpoint hostnames; GCP instance internal/NAT IPs, forwarding rules, and Cloud SQL; Azure NICs and load balancers. Private IPs scope to their network (`GenericPrivateIpAddress`) and resolve across AWS VPC peering, Transit Gateways, Azure VNet peering, and GCP network peering (`docs/network_connectivity_design.md`).
- **Derived edges (`atlas::derive::all`):** `Covers` (host IP in CIDR, per network) and `Serves` (`inferred`/`confirmed` service topology across instance and IP target groups, gated on `scanned_instances`).
- **`atlas-server` & `atlas-web`:** `/snapshot.json` (v3), `/collection.json`, `/ws`; Wasm ForceAtlas2 layout + Sigma.js WebGL with traffic overlay, `Serves` status styling, and service view.
- **`cargo xtask`:** `dev [--demo]`, `wasm`, `demo`, `test [--e2e]`.

## Next Up

1. **Network connectivity Phase 2** — VPNs, Direct Connect, ExpressRoute, and Cloud Interconnect stitched across clouds via public tunnel-endpoint pivots (`docs/network_connectivity_design.md`).
2. **Tiers 1–2 for GCP and Azure** — Cloud Asset Inventory → Pub/Sub, Azure Event Grid, and GCP/Azure flow logs into `FlowObservation` (`docs/change_monitoring_design.md`).
3. **Rendering Phase 3 remainder & Phase 4** — tooltips, drill-down, raw observation values, and search (`docs/graph_rendering_design.md`).

## Backlog

- **Per-node provenance:** Required for cross-tier ordering (preventing a scan from overwriting a newer mid-scan event) and finer-grained outage retention (currently per provider).
- **WebSocket backpressure:** Per-client patch coalescing under high churn.
- **`cargo xtask dev` flag forwarding:** Forward `--aws-event-queue`, `--aws-flow-log-queue`, `--retain-scans`, and `--flow-ttl-secs`.
- **GCP and Azure `CHAINS` rows:** Add GCP backend-service and Azure load-balancer backend-pool kinds for `inferred` `Serves` edges.
- **Route 53 private zones & record identity:** Scope private answers to associated VPCs (`GetHostedZone`) rather than the record name; include record type/zone in `AwsRoute53RecordSet` identity so same-name records (e.g. `A` + `TXT`, or split-horizon zones) do not collide.
- **NIC-only Azure NSGs:** Collect NIC `networkSecurityGroup` references so NSGs not attached to a subnet inherit the NIC's VNet scope.
- **Stopped-instance ENIs:** A stopped instance's ENI keeps its IP in the graph, so traffic still hitting that IP derives `Serves` to `AwsEc2Eni` (`docs/service_topology_design.md` §1).
- **Demo follow-ups (F1–F6):** Scripted 5-step story, interconnect flows, isolated guard nodes, short labels, `/collection.json` UI badge (`docs/demo_design.md`).

## Resume

```bash
cargo xtask test [--e2e]
cargo xtask dev --demo
cargo run --example demo
```

Context: [`CLAUDE.md`](../CLAUDE.md), [`docs/change_monitoring_design.md`](change_monitoring_design.md), [`docs/service_topology_design.md`](service_topology_design.md), [`docs/network_connectivity_design.md`](network_connectivity_design.md), [`docs/demo_design.md`](demo_design.md), [`docs/audit_findings.md`](audit_findings.md).
