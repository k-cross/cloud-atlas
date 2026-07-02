---
name: add-collector
description: Add a new resource type collector to an existing cloud provider in cloud-atlas. Scaffolds the collector module, projector mapping, Node/Edge variants, and a fake-env test.
disable-model-invocation: false
---

When asked to add a new resource type collector for an existing cloud provider, follow these steps in order. Ask which provider and resource type if not specified.

## Step 1 — Add Node variant(s) to `definition.rs`

In `atlas-lib/src/atlas/definition.rs`:
- Add a new `Node::<Provider><ResourceType>(std::sync::Arc<str>)` variant to the `Node` enum in the correct provider section.
- Add the corresponding `Display` arm to the `impl fmt::Display for Node` block, using the `Provider::SubType(id)` format pattern already present in the file.
- Add the variant to the `kinds!(Node, ...)` list at the bottom of the file — the exhaustive match makes skipping this a compile error.
- Add any new `Edge` variants needed (rare), with a `Display` arm and a `kinds!(Edge, ...)` entry.

## Step 2 — Create the collector module

Create `atlas-lib/src/cloud/<provider>/<resource>.rs`. Model it after an existing collector in that provider's directory. Key requirements:
- The function signature must match what `provider.rs` expects (async, returns `Vec<ResourceType>` or similar).
- Use `self.get()` (not `self.client.get()`) for all HTTP calls in GCP API modules to include the auth token.
- Do not use `unwrap()` — use `?` or log and skip errors with the `add_if_ok` pattern from the engine.

## Step 3 — Register the collector in `provider.rs`

In `atlas-lib/src/cloud/<provider>/provider.rs`:
- Add the module import if using a separate file.
- Call the new collector inside the existing async fan-out (alongside the other `tokio::join!` or `try_join_all` calls so it runs concurrently).
- Add the results to the `Provider::<Name> { ... }` struct.

## Step 4 — Add `mod` declaration

In `atlas-lib/src/cloud/<provider>/mod.rs`, add `pub mod <resource>;` (or `mod <resource>;` if it's private).

## Step 5 — Add projector mapping

In `atlas-lib/src/atlas/projector/<provider>.rs`:
- Add a new arm to the resource-matching block (or a new loop) that iterates over the collected resources and calls `graph_builder.get_or_add_node(Node::<NewType>(id.into()))`.
- Wire edges to parent nodes (VPC, subnet, ENI, etc.) using the appropriate `Edge` variant.
- For resources that expose IP addresses or hostnames, stitch them to `Node::GenericIpAddress` / `Node::GenericHostname` via `Edge::RoutesTo` or `Edge::ResolvesTo`.

## Step 6 — Add fixture data and assertions

- In `atlas-lib/src/fixtures.rs`, add at least one instance of the new resource to the provider's fixture function (this is what makes the exhaustiveness guard tests pass).
- In `atlas-lib/src/atlas/tests.rs`, add semantic assertions to the provider's `<provider>_projection` test using the `assert_edge` / `assert_has_node` helpers.
- If the resource exposes an IP or hostname, give the fixture a value that matches another cloud's fixture so the cross-cloud merge is exercised, and assert it in `multi_cloud_seams_merge`.

## Step 7 — Verify

```bash
cargo build
cargo test                     # includes the every_*_kind exhaustiveness guards
cargo run --example demo       # coverage table must show the new kind, exit 0
cargo clippy
```
