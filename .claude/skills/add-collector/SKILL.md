---
name: add-collector
description: Add a new resource type collector to an existing cloud provider in cloud-atlas. Scaffolds the collector module, projector mapping, Node/Edge variants, and a fake-env test.
disable-model-invocation: false
---

Add a resource type to an existing provider. Ask for the provider and resource type if not given. Follow all rules in `CLAUDE.md`.

1. **Node variant (`atlas-lib/src/atlas/definition.rs`):** Add `<Provider><Type>(id) => "Provider::SubType({id})"` under the authoritative `Some(CollectionSource::<Provider>)` group in `nodes!`. Identity only—no mutable state. (If adding a rare `Edge` variant, update `Display`, `kinds!(Edge, ..)`, and `Edge::is_projected()`.)
2. **Collector (`atlas-lib/src/cloud/<provider>/<resource>.rs`):** Return `Result<T, Box<dyn std::error::Error>>` with natural type `T` (multi-field payloads use a named struct in `cloud/definition.rs`). Use `GoogleApiClient::paginated_list`, `CloudflareApiClient::get`/`get_paged`, `cloudflare::paginate`, or the AWS SDK's `into_paginator().items()`. Never stop on a short page, never `unwrap()`, and never swallow errors with `.ok()`. Export in `mod.rs`.
3. **Registration (`cloud/<provider>/provider.rs`):** Add `"name" => <Provider>Collection::Variant, <resource>::collector::runner(..)` to `collectors!` (or add to `azure_types!` / `leaf!` in `azure/provider.rs`). For AWS, add the Config type to `TYPED_CONFIG_TYPES` in `projector/aws.rs` (with the item field an event names it by, or `None`) so Config's catch-all never redraws it.
4. **Projector (`atlas-lib/src/atlas/projector/<provider>.rs`):** Link via `link_to`/`link_from` or `project_leaf!`. Pivot networking on ENIs (`Instance -HasIp-> ENI -AttachedTo-> Subnet`); build IP/hostname pivots via `Node::ip`/`Node::hostname` (`ConnectsTo` = holds IP, `ResolvesTo` = DNS, `RoutesTo` = traffic/rules); gate cross-collection AWS instance references on `scanned_instances`.
5. **Tests & verification:**
   - Collector replay test (`atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs` with `wiremock`, or `cloud/amazon/collector_tests.rs` with `StaticReplayClient`), asserting projector-consumed fields.
   - Add resource to `fixtures.rs` (required by exhaustiveness guards) and assert projection in `atlas/tests.rs`.
   - Run `cargo nextest run --all-targets`, `cargo run --example demo`, and `cargo clippy`.
