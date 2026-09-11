---
name: add-collector
description: Add a new resource type collector to an existing cloud provider in cloud-atlas. Scaffolds the collector module, projector mapping, Node/Edge variants, and a fake-env test.
disable-model-invocation: false
---

When asked to add a new resource type collector for an existing cloud provider, follow these steps in order. Ask which provider and resource type if not specified.

Read `CLAUDE.md` first — the architecture rules there (identity vs. state, failure vs. absence, ENI as the networking pivot) are non-negotiable, and this skill assumes them.

## Step 1 — Add Node variant(s) to `definition.rs`

In `atlas-lib/src/atlas/definition.rs`:
- Add a new `Node::<Provider><ResourceType>(std::sync::Arc<str>)` variant to the `Node` enum in the correct provider section.
- Add the corresponding `Display` arm to the `impl fmt::Display for Node` block, using the `Provider::SubType(id)` format pattern already present in the file.
- Add the variant to the `owned_kinds!(Node, ...)` list at the bottom of the file, **under the `Some(CollectionSource::<Provider>)` group whose scan is authoritative for it**. That grouping generates `kind()`, `ALL_KINDS` and `owner()`, and `owner()` is what scopes carry-forward when a scan comes back incomplete — filing a variant under the wrong provider means an outage in the wrong cloud protects it. The exhaustive match makes skipping this a compile error.
- Add any new `Edge` variants needed (rare), with a `Display` arm and a `kinds!(Edge, ...)` entry.
- The variant keys **identity only**. A field that changes while the resource stays the same (a counter, a `last_seen`, a status) must not go in the enum — it belongs beside the graph, keyed by `node_key`.

## Step 2 — Create the collector module

Create `atlas-lib/src/cloud/<provider>/<resource>.rs`. Model it after an existing collector in that provider's directory. Key requirements:
- Expose a `collector::runner(..)` (AWS) or a `call(..)` (GCP) returning `Result<T, Box<dyn std::error::Error>>`, where `T` is the collector's own natural type (`Vec<Instance>`, `AWSNetworking`) — a collector never names the collection enum itself.
- Multi-field payloads need a *named* struct in `cloud/definition.rs` (see `AWSLoadBalancing`, `AWSRoute53`, `AWSNetworking`), because only a path can be applied as a function by the registration macro.
- GCP: go through `GoogleApiClient::paginated_list` in `api/google/client.rs` — it handles auth, paging and errors. Cloudflare raw REST: `CloudflareApiClient::get` / `get_paged`; page-numbered crate endpoints: `cloudflare::paginate`.
- Never terminate a page loop on "the page came back short" — a clamped `per_page` makes that an ordinary response, and stopping there truncates the collection into what the differ reads as mass deletion.
- Do not use `unwrap()`, and never swallow an error with `.ok()` or `if let Ok(..)` — return it so the caller can record it on the `CollectionReport`. A failed fetch that reaches the projector as an empty collection is read by the differ as deletion.

## Step 3 — Register the collector in `provider.rs`

In `atlas-lib/src/cloud/<provider>/provider.rs`, add **one line** to the `collectors!` macro list:

```rust
"dynamodb" => AmazonCollection::AmazonDynamoDb, dynamodb::collector::runner(&config),
```

The list is handed to `cloud::collector::run_all`, which runs everything concurrently and keeps each collector's name attached to its outcome, so a failure lands on the report as `{scope}/{name}`. Because the line carries the variant *and* the call, registration is type-checked: a collector registered against a variant its return type does not fit fails to compile. Do not hand-roll a `tokio::join!` fan-out.

Add the matching variant to the provider's collection enum in `cloud/definition.rs` if it is a new kind of payload. Azure is different: add the resource type to the `azure_types!` list in `azure/provider.rs`, which feeds both the ARG `where type in~ (..)` filter and `map_resources`' exhaustive dispatch (`leaf!` covers the `{id, name, location}` case in one line).

## Step 4 — Add `mod` declaration

In `atlas-lib/src/cloud/<provider>/mod.rs`, add `pub mod <resource>;` (or `mod <resource>;` if it's private).

## Step 5 — Add projector mapping

In `atlas-lib/src/atlas/projector/<provider>.rs`:
- Add an arm that iterates over the collected resources and links each one in with `graph_builder.link_to(parent, Node::<NewType>(id.into()), Edge::Contains)` / `link_from(..)` — the one-call "get-or-add this node and connect it" helper. Use `get_or_add_node` / `get_or_add_ref` directly only when you need the `NodeIndex` for more than one edge.
- `project_leaf!` in `projector/{azure,gcp}.rs` covers resources that only add a standalone node.
- Wire edges to parent nodes (VPC, subnet, ENI, etc.) using the appropriate `Edge` variant. Networking paths pivot on the ENI: `Instance -> HasIp -> ENI -> AttachedTo -> Subnet`.
- For resources that expose IP addresses or hostnames, stitch them to `Node::GenericIpAddress` / `Node::GenericHostname` via `Edge::RoutesTo` or `Edge::ResolvesTo`. Those nodes dedup automatically, which is what makes cross-cloud merging work.

## Step 6 — Add collector and projection tests

- **Collector test** (HTTP → struct, no credentials): reqwest-based providers get a `wiremock` test in `atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs` via the client's `with_base_url` seam; AWS collectors get a `StaticReplayClient` test in `cloud/amazon/collector_tests.rs`. **Assert the specific fields the projector reads are populated** — our models are all `Option<T>` and serde ignores unknown fields, so a mismatched struct parses into all-`None` and passes a weak "did it parse?" check.
- **Fixtures**: in `atlas-lib/src/fixtures.rs`, add at least one instance of the new resource to the provider's fixture function. This is what makes the exhaustiveness guard tests pass.
- **Projection assertions**: in `atlas-lib/src/atlas/tests.rs`, add semantic assertions to the provider's `<provider>_projection` test using the `assert_edge` / `assert_has_node` helpers.
- If the resource exposes an IP or hostname, give the fixture a value that matches another cloud's fixture so the cross-cloud merge is exercised, and assert it in `multi_cloud_seams_merge`. Generic-node identity is byte-exact — the strings must match exactly.

## Step 7 — Verify

```bash
cargo build
cargo nextest run --all-targets   # includes the every_*_kind exhaustiveness guards
cargo run --example demo          # coverage table must show the new kind, exit 0
cargo clippy
cargo xtask test                  # full gate, if the change reaches the frontend
```
