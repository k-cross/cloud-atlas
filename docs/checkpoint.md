# Checkpoint — 2026-09-16

## State

All four clouds collect; all three change-detection tiers exist for AWS; the whole stack runs credential-free.

- **`atlas-lib`** — collectors and projectors for AWS, GCP, Azure and Cloudflare; 70 `Node` kinds, 10 `Edge` kinds, exhaustiveness guards on both.
- **Tier 3** — `patch::diff` over a persistent graph; `CollectionReport` + `carry_forward` + `Retention` keep failures distinct from deletions.
- **Tier 1** — `atlas::event` + `cloud/amazon/events.rs` (Config, EC2 state, CloudTrail over SQS), including ENI Config items.
- **Tier 2** — `atlas::flow` + `cloud/amazon/flow_logs.rs` (VPC Flow Logs → S3 → SQS).
- **Address coverage (AWS)** — `DescribeNetworkInterfaces` makes every ENI a node with its addresses; RDS instances link to their endpoint hostname.
- **Derived edges** — `atlas::derive::all`: `Covers` (address in CIDR) and `Serves` (`inferred`/`confirmed` service topology).
- **`atlas-server`** — `/snapshot.json` (v3), `/collection.json`, `/ws`.
- **`atlas-web`** — Sigma/WebGL; traffic overlay, `Serves` styling, service view.
- **`cargo xtask`** — `dev [--demo]`, `wasm`, `demo`, `test [--e2e]`.

## Next up

1. **Tiers 1–2 for GCP and Azure** — Cloud Asset Inventory → Pub/Sub, Event Grid, and their flow logs into `FlowObservation`. Cloudflare stays on polling.
2. **Address coverage for GCP and Azure** (service topology 1d) — GCP `network_ip`/`nat_ip`; Azure network interfaces and load balancers, which also unlock their `CHAINS` rows.
3. **IP targets in chains** — `[LB, TargetGroup, <address>]` via the ownership map, so an NLB fronting RDS gets an `inferred` edge.
4. **Rendering phase 3 remainder** — tooltips, drill-down, readable observation values. Phase 4 (search) untouched.

### Backlog

- **Cross-tier ordering** — a scan can't tell that part of its result predates an applied event; needs per-node provenance.
- **Retention granularity** — per provider today, so a long outage releases the whole estate at once; also needs provenance.
- **Backpressure** — a change burst must not stall the push hub; may need per-client coalescing.
- **`cargo xtask dev`** doesn't forward live-feed flags; run `atlas-server` directly.
- **Route 53 record-set identity** — keyed by name alone, so an A and a TXT record for one name, or split-horizon zones, collapse into one node.

## Resume

```bash
cargo xtask test [--e2e]
cargo xtask dev --demo
cargo run --example demo
```

Context: `CLAUDE.md`, `docs/change_monitoring_design.md`, `docs/service_topology_design.md`, `docs/audit_findings.md`.
