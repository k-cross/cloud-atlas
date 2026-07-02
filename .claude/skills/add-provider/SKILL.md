---
name: add-provider
description: Scaffold a brand new cloud provider in cloud-atlas — cloud/ directory, projector, AtlasEngine integration, CLI flag, and fake-env test. Ask which provider if not specified.
disable-model-invocation: false
---

When asked to add a new cloud provider, follow these steps in order. Ask for the provider name and at least one initial resource type if not specified.

## Step 1 — Add Node and Edge variants to `definition.rs`

In `atlas-lib/src/atlas/definition.rs`:
- Add a new section comment (e.g., `// Hetzner`) to the `Node` enum.
- Add `Node::<Provider><ResourceType>(std::sync::Arc<str>)` variants for each resource type.
- Add `Display` arms following the `Provider::SubType(id)` format pattern.
- Add every new variant to the `kinds!(Node, ...)` list at the bottom of the file — the exhaustive match makes skipping this a compile error.
- Add new `Edge` variants only if the existing set (`Contains`, `ConnectsTo`, `AttachedTo`, `HasIp`, `RoutesTo`, `ResolvesTo`, `DependsOn`) doesn't cover the needed relationships (update `kinds!(Edge, ...)` too).

## Step 2 — Scaffold the cloud collection layer

Create `atlas-lib/src/cloud/<provider>/`:
- `mod.rs` — re-exports `provider` and resource modules.
- `provider.rs` — contains `build_<provider>(opts: &Settings) -> Provider` (async). Fan out all resource collectors concurrently with `tokio::join!` or `futures::future::try_join_all`. Return `Provider::<Name> { field1, field2, ... }`.
- One file per resource type (e.g., `instance.rs`, `network.rs`).

For HTTP-based APIs, look at `atlas-lib/src/api/google/client.rs` or `atlas-lib/src/cloud/cloudflare/` as reference patterns.

## Step 3 — Add the provider variant to `cloud/definition.rs`

In `atlas-lib/src/cloud/definition.rs`:
- Add a `Provider::<Name> { field1: Vec<ResourceType>, ... }` arm to the `Provider` enum.

## Step 4 — Add CLI settings

In `atlas-cli/src/main.rs` (or wherever `Settings` is defined):
- Add a `--<provider>` flag and any required options (e.g., `--<provider>-token`, `--<provider>-projects`).
- Add the field to `Settings` and wire it through to `build_<provider>`.

## Step 5 — Create the projector

Create `atlas-lib/src/atlas/projector/<provider>.rs`:
- Implement `pub fn project(provider: &cloud::definition::Provider, graph_builder: &mut GraphBuilder)`.
- Match on `Provider::<Name> { fields }`.
- For each resource, call `graph_builder.get_or_add_node(Node::<Type>(id.into()))` and add edges.
- For any resource that exposes an IP or hostname, add `Node::GenericIpAddress` / `Node::GenericHostname` nodes connected via `Edge::RoutesTo` or `Edge::ResolvesTo`.

## Step 6 — Register the projector

In `atlas-lib/src/atlas/projector/mod.rs`:
- Add `mod <provider>;` and call `<provider>::project(p, &mut graph_builder)` inside the main `build` function's provider-dispatch loop.

## Step 7 — Call `build_<provider>` from the engine

In `atlas-lib/src/atlas/engine.rs`:
- Import and call `build_<provider>(&opts)` alongside the other providers.
- Add the result to the `providers` vec passed to the projector.

## Step 8 — Add fixtures and tests

- In `atlas-lib/src/fixtures.rs`, add a `pub fn <provider>() -> Provider` that populates **every** collection variant of the new provider with at least one resource, register it in `all()`, and give IP/hostname values that match another cloud's fixture so cross-cloud merging is exercised.
- In `atlas-lib/src/atlas/tests.rs`, add a `<provider>_projection` test with semantic `assert_edge` / `assert_has_node` assertions, and extend `multi_cloud_seams_merge` for the new seams.
- The `every_node_kind_appears_in_fixture_graph` guard test will fail until every new node kind is actually produced.

## Step 9 — Verify

```bash
cargo build
cargo test                     # includes the every_*_kind exhaustiveness guards
cargo run --example demo       # coverage table must show the new kinds, exit 0
cargo clippy
```
