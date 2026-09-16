---
name: add-collector
description: Add a new resource type collector to an existing cloud provider in cloud-atlas. Scaffolds the collector module, projector mapping, Node/Edge variants, and a fake-env test.
disable-model-invocation: false
---

Add a resource type to an existing provider. Ask for the provider and resource type if not given. Read `CLAUDE.md` first; its architecture rules are assumed.

## 1. Node variant — `atlas-lib/src/atlas/definition.rs`

- Add one `nodes!` line under the `Some(CollectionSource::<Provider>)` group whose scan is authoritative: `<Provider><Type>(id) => "Provider::SubType({id})"`. That line generates the variant, `Display`, `kind()`, `ALL_KINDS` and `owner()`; the wrong group means an outage in the wrong cloud retains it.
- New `Edge` variants are rare: add the variant, its `Display` arm, a `kinds!(Edge, ..)` entry, and a side in `Edge::is_projected()`.
- Variants hold identity only. Counters, timestamps and status go beside the graph, keyed by `node_key`.

## 2. Collector — `atlas-lib/src/cloud/<provider>/<resource>.rs`

- Model it on a sibling. Return `Result<T, Box<dyn std::error::Error>>` where `T` is the natural type (`Vec<Instance>`, `AWSNetworking`); never name the collection enum. Multi-field payloads need a named struct in `cloud/definition.rs`.
- GCP: `GoogleApiClient::paginated_list`. Cloudflare: `CloudflareApiClient::get`/`get_paged`, or `cloudflare::paginate` for crate endpoints.
- Never stop paging on a short page. No `unwrap()`; never swallow errors with `.ok()`/`if let Ok(..)` — return them for the `CollectionReport`.
- Add `pub mod <resource>;` to the provider's `mod.rs`.

## 3. Register — `cloud/<provider>/provider.rs`

One line in `collectors!`, e.g.:

```rust
"dynamodb" => AmazonCollection::AmazonDynamoDb, dynamodb::collector::runner(&config),
```

The macro feeds `cloud::collector::run_all` and type-checks the variant against the return type. Add the collection variant in `cloud/definition.rs` if needed. **Azure** instead: add the type to `azure_types!` (drives the ARG filter and `map_resources`' exhaustive match; `leaf!` covers `{id, name, location}`).

## 4. Projector — `atlas-lib/src/atlas/projector/<provider>.rs`

- Link with `builder.link_to(parent, Node::<Type>(id.into()), Edge::Contains)` / `link_from(..)`; `get_or_add_node` only when the index is reused.
- `project_leaf!` for node-only resources.
- Networking pivots on the ENI: `Instance -HasIp-> ENI -AttachedTo-> Subnet`.
- Addresses and hostnames: `Node::ip(..)`/`Node::hostname(..)` only. `ConnectsTo` if the resource holds the address, `ResolvesTo` for DNS, `RoutesTo` for traffic/rules.
- AWS: an edge naming an instance from another collection must be gated on `scanned_instances`.

## 5. Tests

- **Collector**: wiremock in `atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs` via `with_base_url`; AWS via `StaticReplayClient` in `cloud/amazon/collector_tests.rs`. Assert the fields the projector reads are populated.
- **Fixtures**: add the resource to the provider's function in `atlas-lib/src/fixtures.rs` (the exhaustiveness guards require it). Give addresses/hostnames a value that matches another cloud's fixture and assert the seam in `multi_cloud_seams_merge`.
- **Projection**: `assert_edge`/`assert_has_node` in the provider's test in `atlas-lib/src/atlas/tests.rs`.

## 6. Verify

```bash
cargo nextest run --all-targets   # includes the exhaustiveness guards
cargo run --example demo          # new kind in the coverage table, exit 0
cargo clippy
cargo xtask test                  # if the change reaches the frontend
```
