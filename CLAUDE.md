# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Cloud Atlas builds a **continuous live property graph** of multi-cloud infrastructure. The overarching goal is a live in-memory digital twin synchronized via event streams — not a static point-in-time snapshot. Keep long-running daemon execution in mind when writing code.

## Architecture Rules

1. **Always use strongly typed enums.** The graph is `petgraph::Graph<Node, Edge>`. All node and edge types are defined in `atlas-lib/src/atlas/definition.rs`. Never use raw strings or hashmaps to represent resources.
2. **ENI is the core networking pivot.** Semantic paths start from the Elastic Network Interface: `Instance -> HasIp -> ENI -> AttachedTo -> Subnet`.
3. **`Display` is required on every new type.** Every new `Node` or `Edge` variant must implement `std::fmt::Display` for clean `.dot` output. Follow the existing `Type::SubType(id)` format pattern.
4. **Cross-cloud stitching via generic nodes.** Use `Node::GenericIpAddress` and `Node::GenericHostname` as cross-cloud integration points. Connect to them with `Edge::RoutesTo` (traffic) or `Edge::ResolvesTo` (DNS). Graph deduplication is automatic — `GraphBuilder` merges identical generic nodes from different clouds via its `HashMap<Node, NodeIndex>`.

## Testing Without Cloud Credentials

No live cloud credentials are available locally. All projection testing runs against the fake "Globex" environment in `atlas-lib/src/fixtures.rs`, which populates **every collection variant of every provider** plus deliberate cross-cloud seams. Do not write tests that require real cloud API calls.

- `cargo test` — includes exhaustiveness guards: every `Node`/`Edge` kind must appear in the fixture graph. Adding an enum variant forces an update to the `kinds!` list in `definition.rs` (compile error otherwise), and the guard test then fails until fixtures + a projector actually produce it.
- `cargo run --example demo` — credential-free verification simulation: projects the fixtures, writes `multi_cloud_demo.dot`, prints a per-kind coverage table, exits non-zero if any kind is missing.

When adding a resource type: add the `Node` variant + `Display` + `kinds!` entry, the projector mapping, and fixture data — the guard tests enforce all three.

## Build & Run

```bash
cargo build --release          # binary: target/release/atlas
cargo run -- --regions us-east-1 us-west-2
cargo run -- --gcp-projects my-project
cargo run -- --cloudflare      # requires CLOUDFLARE_API_TOKEN env var
cargo run -- --azure-subscriptions sub-id
cargo run -- --daemon          # polls every 60 seconds
cargo run -- --all --verbose   # include all mappings, verbose output
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
cargo xtask test [--e2e]       # every suite in order: cargo (root) → cargo (atlas-render) →
                               #   bun test → typecheck [→ playwright]
```

`xtask` (root workspace, alias in `.cargo/config.toml`) only shells out to the same commands listed below — it adds ordering, a readiness gate (frontend waits for `/snapshot.json`), staleness checking for the wasm engine, and teardown of the whole process tree.

## Live Server (`atlas-server/`)

`atlas-cli` is the batch/one-shot path. `atlas-server` is the long-running **live backend** (Phase 2 of `docs/change_monitoring_design.md`): it owns a persistent in-memory graph, reconciles it against the providers on an interval (Tier-3 polling, reusing `AtlasEngine::collect`), diffs each scan (`atlas::patch::diff`), and pushes incremental `GraphPatch`es to the frontend over WebSocket. It never wipes the graph — the differ is the incremental path the daemon lacks.

```bash
cargo run -p atlas-server -- --demo                  # credential-free: serves Globex fixtures
                                                     #   with a churning sentinel, port 4681
cargo run -p atlas-server -- --regions us-east-1     # real collection (same flags as the CLI)
cargo run -p atlas-server -- --poll-secs 30 --port 8080
```

- `GET /snapshot.json` — full current snapshot (v2). `GET /ws` — WebSocket hub.
- WS is **bidirectional**: server pushes `snapshot` then `patch`es; the client can pull `get_snapshot` / `get_neighbors` on demand.
- Point the frontend at it: run `bun dev` in `atlas-render/atlas-web/` (assets on :4680) which connects by default to `ws://<host>:4681/ws`; override with `?server=ws://…` or force offline with `?static`.

## Rendering Workspace (`atlas-render/`)

Interactive rendering (`docs/graph_rendering_design.md`) lives in a **separate cargo workspace** — `atlas-render/` is `exclude`d from the root workspace and must never depend on `atlas-lib` (the cloud SDK tree doesn't build for wasm, and rendering stays decoupled from graph building). The only contract is the versioned render snapshot JSON (and the `GraphPatch` delta of the same shape). It now has **three consumers** that pin `SNAPSHOT_VERSION`: the producer `atlas-lib/src/atlas/export.rs`, the Rust layout consumer `atlas-render/atlas-layout/src/graph.rs`, and the TS frontend `atlas-render/atlas-web/src/graph.ts`. When the shape changes, **bump the version in all three and rebuild the wasm** (`bun run wasm` in `atlas-render/atlas-web/`) — the compiled layout engine bakes in the version and rejects mismatched snapshots at runtime.

- `atlas-layout` — pure-Rust ForceAtlas2 (Barnes-Hut, deterministic, flat `f32` position buffer); `parallel` feature enables rayon natively.
- `atlas-layout-wasm` — wasm-bindgen bridge; builds with `cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown`.
- `atlas-web` — Sigma.js WebGL frontend, a **bun** app (use bun, not node/npm): `bun install && bun run wasm && bun dev` inside `atlas-render/atlas-web/` serves at `http://localhost:4680`. By default it connects to `atlas-server` over WebSocket (`ws://<host>:4681/ws`) for a live snapshot-then-patches feed; with no server it falls back to a static `/snapshot.json` fetch (or force that with `?static`).
- Test with `cargo test` **inside `atlas-render/`** (the root `cargo test` does not cover it). Static end-to-end without credentials: `cargo run --example demo` (root) → `cargo run --example layout_demo -- ../multi_cloud_demo.json` (in `atlas-render/`) → `bun dev` (view at `http://localhost:4680/?static`). Live end-to-end: `cargo run -p atlas-server -- --demo` (root) + `bun dev` (in `atlas-render/atlas-web/`) to watch patches apply as the demo graph churns.

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

- AWS: `cloud/amazon.rs::load_config(region)` — SDK config is loaded once per region in `provider.rs` and passed as `&SdkConfig` to collectors.
- GCP: `GoogleApiClient::paginated_list` in `api/google/client.rs` — every GCP list endpoint goes through it (handles auth, paging, errors).
- Cloudflare: `api_get` in `cloud/cloudflare/mod.rs` — for raw REST endpoints not covered by the `cloudflare` crate.
- Projectors: `project_leaf!` macro in `projector/{azure,gcp}.rs` for resources that only add a standalone node.
- `GraphBuilder::add_edge` deduplicates identical edges automatically.

`docs/audit_findings.md` records resolved audit findings — patterns to avoid reintroducing.
