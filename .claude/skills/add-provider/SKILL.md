---
name: add-provider
description: Scaffold a brand new cloud provider in cloud-atlas — its own atlas-<provider> crate with collectors and projector, AtlasEngine integration, CLI flag, and fake-env test. Ask which provider if not specified.
disable-model-invocation: false
---

Add a new cloud provider (`CollectionSource` unit for retention, carry-forward, and health reporting). Ask for the provider name and at least one initial resource type if not given. Follow all rules in `CLAUDE.md`.

1. **Collection source (`crates/atlas-graph/src/collection.rs`):** Add `CollectionSource::<Name>` and its `Display` arm.
2. **Nodes (`crates/atlas-graph/src/definition.rs`):** Add a `Some(CollectionSource::<Name>) => [..]` block in `nodes!` with `<Provider><Type>(id) => "Provider::SubType({id})"` (generates variant, `Display`, `kind()`, `ALL_KINDS`, and `owner()`). Identity only—no counters, timestamps, or status. Reuse projected `Edge` kinds (`Contains`, `ConnectsTo`, `DependsOn`, `AttachedTo`, `HasIp`, `RoutesTo`, `ResolvesTo`); never emit `TrafficFlow`, `Covers`, or `Serves`.
3. **Crate (`crates/atlas-<name>/`):** Add it to the root `members` and `[workspace.dependencies]`. Its `Cargo.toml` inherits `version`/`edition`/`authors`/`license`/`repository` from the workspace and depends only on `atlas-graph`, `atlas-provider`, and its own SDK/HTTP crates (versions in the root `[workspace.dependencies]`); never on `atlas-engine` or another provider crate.
   - `lib.rs`: module exports and HTTP client with `with_base_url(token, url)` for wiremock (or an `api/` module, as in `atlas-gcp` and `atlas-azure`).
   - `<resource>.rs`: returns `Result<T, Box<dyn std::error::Error>>`. Never stop paging on a short page; never swallow errors with `.ok()`.
   - `definition.rs`: `<Name>Collection` (named structs for multi-field payloads).
   - `provider.rs`: infallible `pub async fn build_<name>(verbose: bool, opts: &Settings) -> ProviderScan<..>` over its own collections (`atlas_provider::{ProviderScan, Settings}`). Diagnose `FailureKind::Unauthorized` pre-fan-out while errors are typed; fan out via `collectors!` + `atlas_provider::collector::run_all`.
   - `fixtures.rs`: its Globex slice (see step 7).
   - `projector.rs`: `<name>_projector` (using `atlas_provider::projector::project_parallel` for scoped data, `atlas_provider::project_leaf!`, and `link_to`/`link_from`). Build pivots only via `Node::ip`/`Node::hostname` (`ConnectsTo` = holds IP, `ResolvesTo` = DNS, `RoutesTo` = traffic/rules).
4. **Aggregate (`atlas-engine`):** Depend on the new crate, add `Provider::<Name>` (per-scope `Vec<(String, XCollection)>` or flat `Box<XCollection>`) and its `build` arm in `crates/atlas-engine/src/projector.rs`, a `<name>()` constructor wrapping the slice in `crates/atlas-engine/src/fixtures.rs` registered in `all()`, and list the crate in `DEMO_SOURCE_CRATES` (`xtask/src/tasks.rs`).
5. **CLI flags:** Add field to `Settings` (`crates/atlas-provider/src/lib.rs`) and clap flag to both `crates/atlas-cli/src/main.rs` and `crates/atlas-server/src/main.rs`.
6. **Engine (`crates/atlas-engine/src/engine.rs::collect`):** Gate future on `Settings`, wrap the scan with `.map(Provider::<Name>)`, join in `tokio::join!`, and retain empty scans so their `CollectionReport` propagates.
7. **Tests & verification:**
   - Collector wiremock tests in `crates/atlas-<name>/tests/collectors.rs` (contract + pagination + error; assert fields read by projector).
   - Fixture slice in `crates/atlas-<name>/src/fixtures.rs` (`collections()`, every variant); its scan scope joins `settings()` and any seam address another cloud names joins `crates/atlas-provider/src/fixtures.rs`.
   - Projection tests in `crates/atlas-<name>/src/projector/tests.rs` (against its slice, via `<name>_projector`); cross-cloud seams in `crates/atlas-engine/src/tests.rs` (`multi_cloud_seams_merge`).
   - Run `cargo xtask test` and `cargo run --example demo`.
