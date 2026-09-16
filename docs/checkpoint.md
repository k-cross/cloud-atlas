# Checkpoint — 2026-09-15

Where we are and what's next, so work can resume cleanly.

## Current state (what works)

All four clouds collect, all three change-detection tiers are built for AWS, and
the whole stack runs credential-free.

- **`atlas-lib`** — collectors + projectors for AWS, GCP, Azure and Cloudflare;
  70 `Node` kinds and 9 `Edge` kinds, with exhaustiveness guards that fail until
  a new variant has a projector *and* fixture data.
- **Every AWS interface carries its address** — `DescribeNetworkInterfaces`
  (`cloud/amazon/network_interface.rs`) collects every ENI, not just the ones
  attached to a described instance, so load balancer, NAT gateway and RDS
  interfaces are real nodes with real addresses. A flow record's endpoint now
  resolves to infrastructure: `ALB -HasIp-> Eni -ConnectsTo-> ip -TrafficFlow->
  ip <-ConnectsTo- Eni <-HasIp- Instance`. The private IP lives on the interface
  that holds it; the instance-level field is a fallback for an instance that
  reports no interfaces.
- **Containment** — `atlas::containment` links every generic address into the
  CIDR ranges that cover it (`Edge::Covers`), so a flow record confirms the
  security-group rule that permitted it. Derived, not owned: recomputed at every
  graph-finalisation point after `carry_forward` and the flow overlay, so the
  ordinary differ handles its whole lifecycle and `carry_forward` refuses to
  hold it (`Edge::is_projected`).
- **Generic pivots are canonical** — `Node::ip` / `Node::hostname` are the only
  way a `GenericIpAddress`/`GenericHostname` is built, normalizing through
  `util::canonical_address` (`IpAddr` round-trip, `::ffff:` unmapping, CIDR
  prefix re-based) and `util::canonical_hostname` (ASCII-lowercase, trailing dot
  stripped). Unparseable values pass through untouched. Three fixture seams are
  deliberately spelled differently on each side so the guards have teeth.
- **Tier 3 (reconciliation)** — `atlas::patch::diff` over a persistent graph.
  `CollectionReport` keeps "could not read" distinguishable from "is gone", and
  `carry_forward` + `Retention` hold an unreadable source's resources for a
  bounded number of scans (shorter when the failure was a credential refusal).
- **Tier 1 (control-plane events)** — `atlas::event` (`ChangeEvent` +
  `EventApplier`, last-writer-wins on the cloud's record time) and
  `cloud/amazon/events.rs` (Config items, EC2 state changes, CloudTrail
  management events off an SQS queue). `--aws-event-queue`.
- **Tier 2 (liveness overlay)** — `atlas::flow` (`FlowObservation` + the bounded,
  expiring `FlowIndex`) and `cloud/amazon/flow_logs.rs` (VPC Flow Logs → S3 → SQS).
  `--aws-flow-log-queue`. The metrics live beside the graph; `Edge::TrafficFlow`
  is payload-free.
- **`atlas-server`** — single-writer graph behind `RwLock` + `broadcast`;
  `poll::run` is a `select!` over the reconciliation ticker and both live feeds.
  `/snapshot.json` (v3), `/collection.json` (scan + stream + flow health), `/ws`.
- **`atlas-render/atlas-web`** — SvelteKit + Sigma/WebGL frontend. Layout settles
  off-screen then reveals under a pinned `customBBox`, warm-started nodes are
  pinned in the engine (this is what killed the shaking), and the Tier-2 overlay
  is rendered as colored/weighted flow edges, node freshness halos, and animated
  packets on a separate traffic canvas.
- **`cargo xtask`** — one command runs/tests the whole stack (`dev [--demo]`,
  `wasm`, `demo`, `test [--e2e]`); wasm staleness check + readiness gate + teardown.
- **Biome** — format+lint for JS/TS/JSON/CSS (`.svelte` → svelte tooling).

## Tests (all green, all credential-free)

| Suite | Command | Count |
|---|---|---|
| Root workspace | `cargo nextest run --all-targets` | **251** (atlas-lib 204, atlas-server 47) |
| Render workspace | `cargo nextest run --all-targets` in `atlas-render/` | **21** |
| Frontend unit | `bun test src/lib` | **40** |
| Frontend e2e | `bun run test:e2e` | **13** |

Collector coverage (HTTP → struct, by replay): AWS 12 in
`cloud/amazon/collector_tests.rs` plus per-module replays, GCP 11, Cloudflare 10
(both the `CloudflareApiClient` endpoints and the `cloudflare`-crate ones), Azure 7.
**Key rule:** our models are all `Option<T>` and serde ignores unknown fields, so
assert the *fields the projector reads are populated*, not just that parsing
succeeded.

## Recent arc (committed history)

`c060864` server → `cbf43cf` shake fix → `6a8616d` svelte migration →
`d381806` collector tests → `eb2944a` `atlas::collection` contract →
`65ada32` failure handling by enum kind → `429d832`/`5f25cc4` network flows
(Tier 2) → `56351df` → generic-pivot canonicalisation → CIDR containment,
current.

## Next up (in priority order)

1. **Tier 1/2 for the remaining clouds** (Phase 5 of
   `change_monitoring_design.md`): GCP Cloud Asset Inventory feeds → Pub/Sub,
   Azure Event Grid, GCP/Azure flow logs into the same `FlowObservation`.
   Cloudflare stays on fast polling — it has no good push story.
2. **Service topology** (`docs/service_topology_design.md`; Phase 1a built). Flow logs cannot yet say *which resources* are
   talking, because only EC2 instances carry an address — an ALB's traffic rides
   ENIs the scan never sees, and GCP instances drop `network_ip` outright. Phase
   1a landed the AWS `DescribeNetworkInterfaces` collector, so every interface
   now carries its addresses and the ALB path resolves end to end. Next is 1b
   (RDS DNS endpoint), then 1c (the Tier-1 Config ENI arm, now unblocked), then
   Phase 2's derived `Edge::Serves` — `inferred` from the control-plane chain,
   `confirmed` by observed traffic, status beside the graph on the existing
   observation channel, no `SNAPSHOT_VERSION` bump. GCP/Azure address coverage
   is deliberately deferred behind getting AWS right.
3. **Rendering Phase 3 remainder**: metadata tooltips and per-node drill-down.
   Phase 4 (search) is untouched. The Tier-2 overlay ships richer data than the
   UI can currently show — `observations` carries `last_seen`/`packets`/`bytes`/
   `status` per key and the only way to read it is inferring from halo intensity
   and bead density.

### Backlog (flagged during audits, not yet done)

- **Cross-tier ordering** — a scan cannot tell that part of its result is older
  than an event already applied, so the tiers race by design (documented, tested).
  Closing it needs per-node provenance the graph does not carry.
- **Retention granularity** — retention is per `CollectionSource`, so a total
  provider outage releases that provider's whole unconfirmed estate in one patch.
  Collector-level retention is not derivable from the node type (an `AwsEc2Vpc`
  comes from five different AWS collectors); it would need provenance recorded
  during projection.
- **Backpressure** — a large change burst must not stall the push hub; patches
  may need coalescing per client.
- **`cargo xtask dev` does not forward the live-feed flags**
  (`--aws-event-queue`, `--aws-flow-log-queue`, `--retain-scans`,
  `--flow-ttl-secs`); run `atlas-server` directly to use them.

## Getting back up to speed

```bash
cargo xtask test            # full gate: root nextest → render nextest → biome → svelte-check → bun unit
cargo xtask test --e2e      # + playwright (static + live WebSocket)
cargo xtask dev --demo      # run the whole stack, credential-free
cargo run --example demo    # fixtures → multi_cloud_demo.{dot,json} + coverage table
```

Deep context: `CLAUDE.md` (architecture, testing rules, collector-test pattern),
`docs/change_monitoring_design.md` (the three-tier roadmap and why each rule
exists), `docs/service_topology_design.md` (the address-coverage invariant and
the derived `Edge::Serves`), `docs/audit_findings.md` (resolved findings, kept as
patterns not to reintroduce).
