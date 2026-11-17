---
name: add-collector
description: Add a new resource type collector to an existing cloud provider in cloud-atlas. Scaffolds the collector module, projector mapping, Node/Edge variants, and a fake-env test.
disable-model-invocation: false
---

Add a resource type to an existing provider. Ask for the provider and resource type if not given. Follow all rules in `CLAUDE.md`. Each provider is its own crate (`atlas-aws`, `atlas-gcp`, `atlas-azure`, `atlas-cloudflare`); every path below is inside it unless it names another crate.

1. **Node variant (`atlas-graph/src/definition.rs`):** Add `<Provider><Type>(id) => "Provider::SubType({id})"` under the authoritative `Some(CollectionSource::<Provider>)` group in `nodes!`. Identity only—no mutable state. (If adding a rare `Edge` variant, update `Display`, `kinds!(Edge, ..)`, and `Edge::is_projected()`.)
2. **Collector (`src/<resource>.rs`; GCP and Azure keep API models and fetches under `src/api/`):** Return `Result<T, Box<dyn std::error::Error>>` with natural type `T` (multi-field payloads use a named struct in `src/definition.rs`). Use `GoogleApiClient::paginated_list`, `CloudflareApiClient::get`/`get_paged`, `atlas_cloudflare::paginate`, or the AWS SDK's `into_paginator().items()`. Never stop on a short page, never `unwrap()`, and never swallow errors with `.ok()`. Export it from `lib.rs` (or `api/mod.rs`). A new third-party dependency gets its version in the root `[workspace.dependencies]` and `dep.workspace = true` in the provider's manifest.
3. **Registration (`src/provider.rs`):** Add `"name" => <Provider>Collection::Variant, <resource>::collector::runner(..)` to `collectors!` (or add to `azure_types!` / `leaf!` in `atlas-azure/src/provider.rs`), and the variant to `<Provider>Collection` in `src/definition.rs`. For AWS, add the Config type to `TYPED_CONFIG_TYPES` in `atlas-aws/src/projector.rs` (with the item field an event names it by, or `None`) so Config's catch-all never redraws it.
4. **Projector (`src/projector.rs`):** Link via `link_to`/`link_from` or `atlas_provider::project_leaf!`. Pivot networking on ENIs (`Instance -HasIp-> ENI -AttachedTo-> Subnet`); build IP/hostname pivots via `Node::ip`/`Node::hostname` (`ConnectsTo` = holds IP, `ResolvesTo` = DNS, `RoutesTo` = traffic/rules); gate cross-collection AWS instance references on `scanned_instances`.
5. **Tests & verification:**
   - Collector replay test (`atlas-{gcp,cloudflare,azure}/tests/collectors.rs` with `wiremock`, or `atlas-aws/src/collector_tests.rs` with `StaticReplayClient`), asserting projector-consumed fields.
   - Add resource to `atlas-lib/src/fixtures.rs` (required by exhaustiveness guards) and assert projection in `atlas-lib/src/atlas/tests.rs`.
   - Run `cargo nextest run --all-targets`, `cargo run --example demo`, and `cargo clippy`.
