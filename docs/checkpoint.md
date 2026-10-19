# Checkpoint — 2026-09-16

## State

All four clouds collect; all three change-detection tiers exist for AWS; the whole stack runs credential-free.

- **`atlas-lib`** — collectors and projectors for AWS, GCP, Azure and Cloudflare; 72 `Node` kinds, 10 `Edge` kinds, exhaustiveness guards on both.
- **Tier 3** — `patch::diff` over a persistent graph; `CollectionReport` + `carry_forward` + `Retention` keep failures distinct from deletions.
- **Tier 1** — `atlas::event` + `cloud/amazon/events.rs` (Config, EC2 state, CloudTrail over SQS), including ENI Config items.
- **Tier 2** — `atlas::flow` + `cloud/amazon/flow_logs.rs` (VPC Flow Logs → S3 → SQS).
- **Address coverage** — AWS `DescribeNetworkInterfaces` (ENIs carry addresses and owners) and RDS endpoint hostnames; GCP instance internal/NAT addresses; Azure NICs and load balancers. Private addresses are scoped to the network they were read in (`GenericPrivateIpAddress`); only public ones stitch across networks.
- **Derived edges** — `atlas::derive::all`: `Covers` (address in CIDR, per network) and `Serves` (`inferred`/`confirmed` service topology, including instance and IP target-group chains; both kinds of target gated on `scanned_instances`).
- **`atlas-server`** — `/snapshot.json` (v3), `/collection.json`, `/ws`.
- **`atlas-web`** — Sigma/WebGL; traffic overlay, `Serves` styling, service view.
- **`cargo xtask`** — `dev [--demo]`, `wasm`, `demo`, `test [--e2e]`.

## Next up

1. **Tiers 1–2 for GCP and Azure** — Cloud Asset Inventory → Pub/Sub, Event Grid, and their flow logs into `FlowObservation`. Cloudflare stays on polling.
2. **Rendering phase 3 remainder** — tooltips, drill-down, readable observation values. Phase 4 (search) untouched.

### Backlog

- **Cross-tier ordering** — a scan can't tell that part of its result predates an applied event; needs per-node provenance.
- **Retention granularity** — per provider today, so a long outage releases the whole estate at once; also needs provenance.
- **Backpressure** — a change burst must not stall the push hub; may need per-client coalescing.
- **`cargo xtask dev`** doesn't forward live-feed flags; run `atlas-server` directly.
- **GCP and Azure `CHAINS` rows** — need a backend-service kind (GCP) and a backend pool under `AzureLoadBalancer`; until then their `Serves` edges come from traffic alone.
- **Interconnect-aware private addresses** — AWS VPC peering and Transit Gateway, Azure VNet peering, GCP VPC peering, and VPNs or dedicated interconnects make another network's private addresses reachable, and networks joined that way cannot overlap. Collect them, then resolve a private address across the networks its own one is joined to (a derive-time lookup, so identity stays per network). Restores the fixture's two interconnect hops.
- **Route 53 private zones** — a record's private answer is scoped to the record name; `GetHostedZone`'s VPC associations would place it in the VPCs that resolve it.
- **Azure NSGs on NICs** — NSG rules are read in the VNets of the subnets they're attached to; an NSG attached only to NICs is scoped to itself until the NIC's `networkSecurityGroup` is collected.
- **A stopped instance's interface** — stays in the graph holding its address, so a client still sending there gets `Serves` to the interface (`service_topology_design.md` §1).
- **Demo relevance** — tracked in `docs/demo_design.md`: a scripted story in place of the canary record (F1), legible per-network addresses (F2), the filler that pushes the story off-screen (F3), labels (F4), collection health in the UI (F5).
- **Route 53 record-set identity** — keyed by name alone, so an A and a TXT record for one name, or split-horizon zones, collapse into one node.

## Resume

```bash
cargo xtask test [--e2e]
cargo xtask dev --demo
cargo run --example demo
```

Context: `CLAUDE.md`, `docs/change_monitoring_design.md`, `docs/service_topology_design.md`, `docs/demo_design.md`, `docs/audit_findings.md`.
