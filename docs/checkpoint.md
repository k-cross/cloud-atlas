# Checkpoint — 2026-09-11

Where we are and what's next, so work can resume cleanly.

## Current state (what works)

All four clouds collect, all three change-detection tiers are built for AWS, and
the whole stack runs credential-free.

- **`atlas-lib`** — collectors + projectors for AWS, GCP, Azure and Cloudflare;
  69 `Node` kinds and 8 `Edge` kinds, with exhaustiveness guards that fail until
  a new variant has a projector *and* fixture data.
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
| Root workspace | `cargo nextest run --all-targets` | **225** (atlas-lib 181, atlas-server 44) |
| Render workspace | `cargo nextest run --all-targets` in `atlas-render/` | **21** |
| Frontend unit | `bun test src/lib` | **38** |
| Frontend e2e | `bun run test:e2e` | **14** |

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
(Tier 2) → `452b969` current.

## Next up (in priority order)

1. **Tier 1/2 for the remaining clouds** (Phase 5 of
   `change_monitoring_design.md`): GCP Cloud Asset Inventory feeds → Pub/Sub,
   Azure Event Grid, GCP/Azure flow logs into the same `FlowObservation`.
   Cloudflare stays on fast polling — it has no good push story.
2. **Generic-node canonicalisation** (open question in
   `change_monitoring_design.md` §10). `GenericIpAddress` identity is byte-exact,
   so `2001:db8::1` and `2001:0db8:0000:…` are two different pivots and the
   cross-cloud seam silently fails to stitch. Fix at the single construction
   site, leaving anything that will not parse untouched.
3. **CIDR containment for inferred reachability.** A security-group rule projects
   `GenericIpAddress("198.51.100.0/24")`; a flow log projects
   `GenericIpAddress("198.51.100.10")`. Exact matching keeps them apart, so
   "observed traffic confirms an inferred edge" does not actually work yet.
   Needs a prefix trie per scan, plus a decision on what edge kind owns the link.
4. **Rendering Phase 3 remainder**: metadata tooltips and per-node drill-down.
   Phase 4 (search) is untouched.

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
exists), `docs/audit_findings.md` (resolved findings, kept as patterns not to
reintroduce).
