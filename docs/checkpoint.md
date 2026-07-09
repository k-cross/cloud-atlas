# Checkpoint — 2026-07-08

Where we are and what's next, so work can resume cleanly.

## ⚠️ First: uncommitted work

The **collector-testing expansion is uncommitted** (last commit `6d8348f add
collector tests` only holds the 4 *reference* tests). Commit before modifying
the machine, or it's at risk. Uncommitted:

- GCP collectors routed through `endpoint()`: `api/google/{dns,functions,gke,pubsub,run,sql,storage}.rs`
- New tests: `atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs`, `cloud/amazon/collector_tests.rs`
- `cloud/azure/provider.rs` — extracted `map_resources` (pure, testable)
- `cloud/amazon/{security_group,sqs}.rs` — replay tests; `Cargo.toml` dev-deps; `CLAUDE.md`

Everything is green: `cargo test -p atlas-lib` → **57 passing, 0 failed**.

## Current state (what works)

- **`atlas-server`** — live backend (Phase 2 of `change_monitoring_design.md`):
  owns a persistent graph, Tier-3 poll → `atlas::patch::diff` → pushes
  `GraphPatch`es over WebSocket. `--demo` runs credential-free.
- **`atlas-render/atlas-web`** — SvelteKit + Sigma/WebGL frontend, connects to
  the server live (`?static` for offline). Layout **settles off-screen then
  reveals** under a pinned `customBBox`, and warm-started nodes are **pinned in
  the engine** — this is what finally killed the shaking (build + pan/zoom).
- **`cargo xtask`** — one command runs/tests the whole stack (`dev --demo`,
  `test [--e2e]`); wasm staleness check + readiness gate + teardown.
- **Biome** — format+lint for JS/TS/JSON/CSS (`.svelte` → svelte tooling).
- **Tests** — frontend: 16 unit + 11 e2e (shake/pixel-stability, warm-start
  pinning, zero-width, version-mismatch, live patches). Collectors: **42**
  across all four clouds (see below) + 15 projection tests.

## Recent arc (committed history)

`c060864` server → `cbf43cf` shake fix → `6a8616d` svelte migration →
`6d8348f` collector tests → **(uncommitted)** collector fan-out.

## Collector testing — the pattern (credential-free)

`fixtures.rs` tests projectors; collectors (HTTP→struct) are tested by replaying
canned responses. **Key rule:** models are all `Option<T>` + serde ignores
unknowns, so assert *fields are populated*, not just that parsing succeeded.

- reqwest clients (GCP/Cloudflare/Azure): `wiremock` + a `with_base_url` seam.
- AWS SDK: `StaticReplayClient` through the real deserializer (`collector_tests.rs`).

Coverage: **AWS 15/15**, **GCP 14**, **Azure 8**, **Cloudflare 5**.

## Next up (in priority order)

1. **Finish Cloudflare crate collectors** (zone/dns/kv/r2) — they use the
   `cloudflare` crate's own client, not `CloudflareApiClient`, so wiremock
   doesn't apply. Approach: contract tests on their result structs. This closes
   out the collector-testing effort.
2. **Then: liveness (Phase 3+ of `change_monitoring_design.md`)** — the original
   roadmap. Tier-1 cloud event streams (AWS EventBridge/Config first) as the
   primary change feed; Tier-2 flow-log liveness overlay → node freshness +
   `Edge::TrafficFlow`. `GraphPatch`/WS protocol already shaped to carry these.

### Backlog (flagged during audits, not yet done)

- **Projector duplication** — `get_or_add_node`+`add_edge` idiom ~177× across
  `projector/{aws,gcp,azure,cloudflare}.rs`. Add `GraphBuilder::link_to/link_from`
  helpers; guarded by existing projection tests.
- **atlas-server tests** — `ws.rs`/`state.rs`/`poll.rs` only covered indirectly
  via frontend e2e; add `#[tokio::test]` integration for the WS protocol.
- **Frontend note** — `whenSized` no longer gates Sigma construction (it's in the
  `GraphController` constructor now); harmless (relies on `allowInvalidContainer`)
  but the guard is vestigial.

## Getting back up to speed

```bash
cargo xtask test            # full gate: workspace cargo + render + biome + unit
cargo xtask test --e2e      # + playwright (static + live WebSocket)
cargo xtask dev --demo      # run the whole stack, credential-free
cargo test -p atlas-lib     # collector + projection tests (57)
```

Deep context: `CLAUDE.md` (architecture, testing rules, collector-test pattern),
`docs/change_monitoring_design.md` (the live-backend roadmap).
