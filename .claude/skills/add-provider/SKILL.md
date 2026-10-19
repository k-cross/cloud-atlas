---
name: add-provider
description: Scaffold a brand new cloud provider in cloud-atlas — cloud/ directory, projector, AtlasEngine integration, CLI flag, and fake-env test. Ask which provider if not specified.
disable-model-invocation: false
---

Add a new cloud provider (`CollectionSource` unit for retention, carry-forward, and health reporting). Ask for the provider name and at least one initial resource type if not given. Follow all rules in `CLAUDE.md`.

1. **Collection source (`atlas-lib/src/atlas/collection.rs`):** Add `CollectionSource::<Name>` and its `Display` arm.
2. **Nodes (`atlas-lib/src/atlas/definition.rs`):** Add a `Some(CollectionSource::<Name>) => [..]` block in `nodes!` with `<Provider><Type>(id) => "Provider::SubType({id})"` (generates variant, `Display`, `kind()`, `ALL_KINDS`, and `owner()`). Identity only—no counters, timestamps, or status. Reuse projected `Edge` kinds (`Contains`, `ConnectsTo`, `DependsOn`, `AttachedTo`, `HasIp`, `RoutesTo`, `ResolvesTo`); never emit `TrafficFlow`, `Covers`, or `Serves`.
3. **Collectors (`atlas-lib/src/cloud/<provider>/`):**
   - `mod.rs`: module exports and HTTP client with `with_base_url(token, url)` for wiremock.
   - `<resource>.rs`: returns `Result<T, Box<dyn std::error::Error>>`. Never stop paging on a short page; never swallow errors with `.ok()`.
   - `provider.rs`: infallible `pub async fn build_<provider>(verbose: bool, opts: &Settings) -> ProviderScan`. Diagnose `FailureKind::Unauthorized` pre-fan-out while errors are typed; fan out via `collectors!` + `cloud::collector::run_all`.
4. **Provider enum (`atlas-lib/src/cloud/definition.rs`):** Add `Provider::<Name>` (per-scope `Vec<(String, XCollection)>` or flat `Box<XCollection>`) and `<Name>Collection` (named structs for multi-field payloads).
5. **CLI flags:** Add field to `Settings` (`atlas-lib/src/lib.rs`) and clap flag to both `atlas-cli/src/main.rs` and `atlas-server/src/main.rs`.
6. **Projector (`atlas-lib/src/atlas/projector/<provider>.rs`):** Implement `<provider>_projector` (using `project_parallel` for scoped data, `project_leaf!`, and `link_to`/`link_from`). Build pivots only via `Node::ip`/`Node::hostname` (`ConnectsTo` = holds IP, `ResolvesTo` = DNS, `RoutesTo` = traffic/rules). Register in `projector/mod.rs`.
7. **Engine (`atlas-lib/src/atlas/engine.rs::collect`):** Gate future on `Settings`, join in `tokio::join!`, and retain empty scans so their `CollectionReport` propagates.
8. **Tests & verification:**
   - Collector wiremock tests in `atlas-lib/tests/<provider>_collectors.rs` (contract + pagination + error; assert fields read by projector).
   - Fixture data in `fixtures.rs` (all variants + cross-cloud seams) registered in `all()`.
   - Projection tests in `atlas/tests.rs` + `multi_cloud_seams_merge`.
   - Run `cargo xtask test` and `cargo run --example demo`.
