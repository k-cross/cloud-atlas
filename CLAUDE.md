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

Output is written to `atlas.dot` in the working directory (gitignored). Visualize with Gephi.

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
