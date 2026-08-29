# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Cloud Atlas builds a **continuous live property graph** of multi-cloud infrastructure. The overarching goal is a live in-memory digital twin synchronized via event streams — not a static point-in-time snapshot. Keep long-running daemon execution in mind when writing code.

## Architecture Rules

1. **Always use strongly typed enums.** The graph is `petgraph::Graph<Node, Edge>`. All node and edge types are defined in `atlas-lib/src/atlas/definition.rs`. Never use raw strings or hashmaps to represent resources.
2. **ENI is the core networking pivot.** Semantic paths start from the Elastic Network Interface: `Instance -> HasIp -> ENI -> AttachedTo -> Subnet`.
3. **`Display` is required on every new type.** Every new `Node` or `Edge` variant must implement `std::fmt::Display` for clean `.dot` output. Follow the existing `Type::SubType(id)` format pattern.
4. **Never let a failure look like an absence.** This is a live graph, so "we could not read it" and "it is gone" must stay distinguishable all the way to the differ — see the `CollectionReport` contract under Live Server.
5. **Cross-cloud stitching via generic nodes.** Use `Node::GenericIpAddress` and `Node::GenericHostname` as cross-cloud integration points. Connect to them with `Edge::RoutesTo` (traffic) or `Edge::ResolvesTo` (DNS). Graph deduplication is automatic — `GraphBuilder` merges identical generic nodes from different clouds via its `HashMap<Node, NodeIndex>`.

## Testing Without Cloud Credentials

No live cloud credentials are available locally. All projection testing runs against the fake "Globex" environment in `atlas-lib/src/fixtures.rs`, which populates **every collection variant of every provider** plus deliberate cross-cloud seams. Do not write tests that require real cloud API calls.

- `cargo nextest run` — includes exhaustiveness guards: every `Node`/`Edge` kind must appear in the fixture graph. Adding an enum variant forces an update to the `kinds!` list in `definition.rs` (compile error otherwise), and the guard test then fails until fixtures + a projector actually produce it.
- `cargo run --example demo` — credential-free verification simulation: projects the fixtures, writes `multi_cloud_demo.dot`, prints a per-kind coverage table, exits non-zero if any kind is missing.

When adding a resource type: add the `Node` variant + `Display` + `owned_kinds!` entry, the projector mapping, and fixture data — the guard tests enforce all three. `Node`'s list is grouped by owning `CollectionSource` (`owned_kinds!` generates `kind()`, `ALL_KINDS`, and `owner()` from it), so a new variant must be filed under the provider whose scan is authoritative for it — that grouping is what scopes carry-forward on an incomplete scan.

### Collector tests (the HTTP → struct boundary)

Fixtures test **projectors** (they hand-build `Provider` collections), not the **collectors** that fetch and deserialize cloud API responses. Collectors are tested by replaying canned responses — no credentials, no network:

- **reqwest clients (GCP/Cloudflare/Azure):** `wiremock` mock server + a `base_url` seam. Each client has a `with_base_url(token, url)` DI constructor that points every collector at the mock — `GoogleApiClient` (`api/google/compute.rs`), `CloudflareApiClient` (`cloud/cloudflare/worker.rs`), `AzureApiClient` (`api/azure/client.rs`). Each example pairs a Layer-1 contract test (deserialize a realistic body) with a Layer-2 test exercising the real pagination + error path (GCP `nextPageToken`, Azure `$skipToken`, Cloudflare's `{success,result}` envelope).
- **AWS SDK (`aws-sdk-*`):** `aws_smithy_runtime`'s `StaticReplayClient` injected into a test `SdkConfig` via `.http_client(...)` — replays a canned response through the *real* SDK deserializer (AWS types aren't `serde`, so this is the only way to test them). See `cloud/amazon/instance.rs` tests.

Per-collector fan-out: GCP/Cloudflare/Azure wiremock tests live in `atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs`; every AWS collector is covered in `cloud/amazon/collector_tests.rs` (a shared `replay_config` helper feeds canned responses, one per request, in order — must be a unit module since `aws-config` is a normal dep unavailable to integration tests). Azure's `provider::map_resources` is split out from the fetch so the ARG-row → typed-model mapping is testable without `az login`; it returns `(collections, CollectionReport)` and never fails — a row that will not deserialize is skipped and reported (capped at `MAX_REPORTED_ROWS` plus a tail count), because ARG returns the whole tenant in one response and failing the batch on one drifted row would empty the Azure half of the graph.

The `cloudflare`-crate collectors (zone/dns/kv) go through the `cloudflare` crate's own `Client`, not `CloudflareApiClient`, so their seam is `Environment::Custom(format!("{}/client/v4/", server.uri()))` — the crate joins each endpoint's relative `path()` onto that base, so wiremock works there too (same file, `serve_crate`/`crate_client` helpers). Unlike our own all-`Option` models, the crate's result structs are strict (`Zone`, `DnsRecord` have mostly required fields, `DnsContent` is an internally-tagged enum), so a drifted body fails deserialization outright.

Key rule: our own models are all `Option<T>` and serde ignores unknown fields, so a mismatched struct parses into all-`None` and passes a weak "did it parse?" check. **Assert the specific fields the projector reads are populated**, not just that deserialization succeeded.

## Build & Run

```bash
cargo build --release          # binary: target/release/atlas
cargo run -- --regions us-east-1 us-west-2
cargo run -- --gcp-projects my-project
cargo run -- --cloudflare      # requires CLOUDFLARE_API_TOKEN env var
cargo run -- --azure-subscriptions sub-id
cargo run -- --daemon          # polls every 60 seconds
cargo run -- --verbose         # verbose output
```

Output is written to `atlas.dot` plus a render snapshot `atlas.json` in the working directory (both gitignored). Visualize `.dot` with Gephi.

## Dev Orchestration (`cargo xtask`)

The unified way to run and test the whole stack (server + wasm renderer + frontend) — prefer these over hand-running the pieces:

```bash
cargo xtask dev --demo         # full stack, credential-free: wasm (rebuilt if stale) →
                               #   atlas-server :4681 → frontend :4680, one Ctrl-C stops all
cargo xtask dev                # same, real collection (default; add provider flags as needed)
cargo xtask wasm [--force]     # rebuild pkg/ if atlas-layout sources are newer (do this
                               #   after any SNAPSHOT_VERSION bump)
cargo xtask demo               # regenerate multi_cloud_demo.json from fixtures
cargo xtask test [--e2e]       # every suite in order: nextest (root) → nextest (atlas-render) →
                               #   bun test → typecheck [→ playwright]
```

**cargo-nextest is the default Rust test runner.** `cargo xtask test` runs
`cargo nextest run --all-targets` in both workspaces, falling back to
`cargo test --all-targets` when cargo-nextest isn't installed (`cargo install
cargo-nextest --locked`). The printed `▶` line shows which runner was used.
Two things to know:

- `--all-targets` is load-bearing: nextest skips `examples/` by default, and it
  is what keeps `atlas-lib/examples/demo.rs` and `atlas-layout`'s
  `layout_demo.rs` compile-checked the way plain `cargo test` did.
- **nextest never runs doctests.** The repo has none today; if you add one, it
  needs a separate `cargo test --doc`.

Shared config lives in `.config/nextest.toml` — one per workspace (root and
`atlas-render/`), since nextest resolves config from the workspace root. The
`default` profile is fail-fast with a 30s slow-test warning; a `ci` profile
(`cargo nextest run -P ci`) runs the full suite with one retry.

`xtask` (root workspace, alias in `.cargo/config.toml`) only shells out to the same commands listed below — it adds ordering, a readiness gate (frontend waits for `/snapshot.json`), staleness checking for the wasm engine, and teardown of the whole process tree.

## Live Server (`atlas-server/`)

`atlas-cli` is the batch/one-shot path. `atlas-server` is the long-running **live backend** (Phase 2 of `docs/change_monitoring_design.md`): it owns a persistent in-memory graph, reconciles it against the providers on an interval (Tier-3 polling, reusing `AtlasEngine::collect`), diffs each scan (`atlas::patch::diff`), and pushes incremental `GraphPatch`es to the frontend over WebSocket. It never wipes the graph — the differ is the incremental path the daemon lacks.

**Collection outcomes are part of the contract.** `AtlasEngine::collect` returns a `Scan { builder, report }`, where the `CollectionReport` lists every source that could not be read (`atlas::collection`). A failed fetch must never reach the projector as an empty collection: the differ would read the absence as deletion and broadcast a removal for every node that source owns, which the next healthy tick puts straight back. When a report names sources it could not read, `poll::reconcile` folds the live graph forward for **those sources only** (`patch::carry_forward` + `report.unreadable_sources()`), so the tick is additive-only inside the failed provider's territory while every healthy provider stays authoritative, deletions included. That scoping is what makes the graph converge: a collector that fails on every tick must not stop a genuinely deleted resource in *another* cloud from ever being removed. Ownership is `Node::owner()`, generated from the grouped variant list in `definition.rs`; nodes with no owner (`GenericIpAddress`/`GenericHostname`/`ExternalService` — the cross-cloud stitching points) are retained on any incomplete scan, since any provider may still reference them. The CLI daemon applies the identical policy in `AtlasEngine::install` before writing `atlas.dot`/`atlas.json`.

**Retention is a budget, not a promise** (`patch::Retention`). Holding resources is the right answer to a transient failure and the wrong answer to a permanent one: a collector that fails on every tick would pin its resources forever, and "unconfirmed since start-up" is not a live twin. A source is held for `--retain-scans` consecutive incomplete scans (default 10, so ten minutes at the default poll interval) and released on the next one, letting the differ delete what the scans could not confirm; recovery resets the streak. Both the server loop and the CLI daemon run the same `Retention`. Note the bluntness this trades for: a long total outage releases that provider's whole unconfirmed estate in one patch. Finer, collector-level retention is *not* derivable from the node type — an `AwsEc2Vpc` is produced by five different AWS collectors — so it would need per-node provenance recorded during projection. Providers signal this by returning `ProviderScan { provider, report }`. **`build_*` is infallible** — there is no second failure channel: a provider that dies at the credential step still returns a scan, with the empty collection explained by its report (`"credentials"`, `"auth"`), and a partial read returns what it got plus a failure per unreachable scope (a throttled AWS collector, one unreachable Cloudflare zone). Never swallow a collector error with `.ok()` or `if let Ok(..)` — record it on the report.

```bash
cargo run -p atlas-server -- --demo                  # credential-free: serves Globex fixtures
                                                     #   with a churning sentinel, port 4681
cargo run -p atlas-server -- --regions us-east-1     # real collection (same flags as the CLI)
cargo run -p atlas-server -- --poll-secs 30 --port 8080
cargo run -p atlas-server -- --retain-scans 3          # give up on an unreadable
                                                       #   provider after 3 scans
```

- `GET /snapshot.json` — full current snapshot (v2). `GET /collection.json` — whether the last scan was complete plus its attributed failures, which is the only way a client can tell "this provider holds nothing" from "this provider could not be reached" (it matters most at start-up, when an outage makes the first partial collection the baseline). `GET /ws` — WebSocket hub.
- WS is **bidirectional**: server pushes `snapshot` then `patch`es; the client can pull `get_snapshot` / `get_neighbors` on demand.
- Point the frontend at it: run `bun dev` in `atlas-render/atlas-web/` (assets on :4680) which connects by default to `ws://<host>:4681/ws`; override with `?server=ws://…` or force offline with `?static`.

## Rendering Workspace (`atlas-render/`)

Interactive rendering (`docs/graph_rendering_design.md`) lives in a **separate cargo workspace** — `atlas-render/` is `exclude`d from the root workspace and must never depend on `atlas-lib` (the cloud SDK tree doesn't build for wasm, and rendering stays decoupled from graph building). The only contract is the versioned render snapshot JSON (and the `GraphPatch` delta of the same shape). It now has **three consumers** that pin `SNAPSHOT_VERSION`: the producer `atlas-lib/src/atlas/export.rs`, the Rust layout consumer `atlas-render/atlas-layout/src/graph.rs`, and the TS frontend `atlas-render/atlas-web/src/graph.ts`. When the shape changes, **bump the version in all three and rebuild the wasm** (`bun run wasm` in `atlas-render/atlas-web/`) — the compiled layout engine bakes in the version and rejects mismatched snapshots at runtime.

- `atlas-layout` — pure-Rust ForceAtlas2 (Barnes-Hut, deterministic, flat `f32` position buffer); `parallel` feature enables rayon natively.
- `atlas-layout-wasm` — wasm-bindgen bridge; builds with `cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown`.
- `atlas-web` — Sigma.js WebGL frontend, a **bun** app (use bun, not node/npm): `bun install && bun run wasm && bun dev` inside `atlas-render/atlas-web/` serves at `http://localhost:4680`. By default it connects to `atlas-server` over WebSocket (`ws://<host>:4681/ws`) for a live snapshot-then-patches feed; with no server it falls back to a static `/snapshot.json` fetch (or force that with `?static`).
- Test with `cargo nextest run` **inside `atlas-render/`** (the root run does not cover it — it is a separate workspace). Static end-to-end without credentials: `cargo run --example demo` (root) → `cargo run --example layout_demo -- ../multi_cloud_demo.json` (in `atlas-render/`) → `bun dev` (view at `http://localhost:4680/?static`). Live end-to-end: `cargo run -p atlas-server -- --demo` (root) + `bun dev` (in `atlas-render/atlas-web/`) to watch patches apply as the demo graph churns.

## Auth (reference only — not available locally)

| Provider | Mechanism |
|---|---|
| AWS | Standard credential chain (`~/.aws/credentials`, env vars, instance role) |
| GCP | OAuth2 browser flow via `yup-oauth2` — opens browser on first run |
| Cloudflare | `CLOUDFLARE_API_TOKEN` env var — required, hard-errors if missing |
| Azure | `az login` (`AzureCliCredential`) |

## Rust Edition

Rust **2024 edition** (`let`-chain syntax). Requires rustup stable ≥ 1.85.

## VCS

Uses **jj (Jujutsu)** on top of git. Typical workflow: `jj describe` → `jj new` → `jj squash` → `jj git push --change <id>` for stacked PRs on GitHub.

## Established Helpers (use these, don't re-duplicate)

- Per-scope fan-out: `cloud::collector::run_all` — hand it the region's/project's `Vec<NamedCollector<'_, T>>` and it runs them concurrently, keeping each collector's name attached to its outcome so a failure lands in the report as `{scope}/{name}`. Both AWS and GCP build that list with a local `collectors!` macro: one line per collector, no parallel join/destructure to keep in sync.
- AWS: `cloud/amazon.rs::load_config(region)` — SDK config is loaded once per region in `provider.rs` and passed as `&SdkConfig` to collectors. Register a new collector as one `"name" => runner(..)` line in the `collectors!` list in `amazon/provider.rs`.
- GCP: `GoogleApiClient::paginated_list` in `api/google/client.rs` — every GCP list endpoint goes through it (handles auth, paging, errors). Register a new one as `"name" => GoogleCollection::Variant, call(..)` in `google/provider.rs`'s `collectors!` list.
- Azure: `AzureApiClient::query_graph` treats a response with no `data` **array** as an error, never as an empty tenant — table-format results, a missing or null `data`, and an error body delivered with a 2xx all used to return `Ok` with zero rows, which reads as a complete scan of an empty tenant and deletes every Azure node in the graph. It also errors when `totalRecords` claims more records than came back. Keep that asymmetry if you touch it: too few rows is data loss, too many is harmless.
- Azure: the `azure_types!` list in `azure/provider.rs` is the single source for both the ARG `where type in~ (..)` filter and `map_resources`' dispatch. Add a resource type there and the exhaustive match makes the compiler demand its mapping arm; `leaf!` covers the `{id, name, location}` case in one line.
- Cloudflare: `CloudflareApiClient::get` in `cloud/cloudflare/mod.rs` — for raw REST endpoints not covered by the `cloudflare` crate (`get_paged` also hands back `result_info`, which cursor-paginated endpoints like R2 need). `cloudflare::paginate` walks any page-numbered crate endpoint to exhaustion: pass `per_page` and a closure taking the page number. Never terminate a page loop on "the page came back short" — a clamped `per_page` makes that an ordinary response, and stopping there silently truncates the collection into what the differ reads as mass deletion.
- Projectors: `project_leaf!` macro in `projector/{azure,gcp}.rs` for resources that only add a standalone node.
- `GraphBuilder::add_edge` deduplicates identical edges automatically, and `GraphBuilder::merge(&graph)` is the single definition of folding one graph into another by node identity — `patch::carry_forward` is a thin policy wrapper over it, so never hand-roll a node/edge dedup pass.

`docs/audit_findings.md` records resolved audit findings — patterns to avoid reintroducing.
