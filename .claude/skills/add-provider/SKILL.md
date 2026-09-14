---
name: add-provider
description: Scaffold a brand new cloud provider in cloud-atlas — cloud/ directory, projector, AtlasEngine integration, CLI flag, and fake-env test. Ask which provider if not specified.
disable-model-invocation: false
---

When asked to add a new cloud provider, follow these steps in order. Ask for the provider name and at least one initial resource type if not specified.

Read `CLAUDE.md` first — the architecture rules there (identity vs. state, failure vs. absence, cross-cloud pivot nodes) are non-negotiable, and this skill assumes them. Adding a provider means adding a new `CollectionSource`, which is the unit that retention, carry-forward and health reporting all work in.

## Step 1 — Add the collection source

In `atlas-lib/src/atlas/collection.rs`, add a `CollectionSource::<Name>` variant with its `Display` arm. Everything downstream — `CollectionReport`, `unreadable_sources()`, `Retention`, `/collection.json` — keys off this.

## Step 2 — Add Node and Edge variants to `definition.rs`

In `atlas-lib/src/atlas/definition.rs`:
- Add a new `Some(CollectionSource::<Name>) => [..]` group to the `nodes!(..)` list, with one line per resource type: `<Provider><ResourceType>(id) => "Provider::SubType({id})"`, following the label pattern already present in the file.
- Those lines are the whole `Node` change — the group declares the variants, their `Display`, and their entries in `kind()`, `ALL_KINDS` and `owner()`; `owner()` is what scopes carry-forward when the new provider's scan comes back incomplete.
- Add new `Edge` variants only if the existing set (`Contains`, `ConnectsTo`, `AttachedTo`, `HasIp`, `RoutesTo`, `ResolvesTo`, `DependsOn`, `TrafficFlow`) doesn't cover the needed relationships (update `kinds!(Edge, ...)` too).
- Variants key **identity only** — never a counter, timestamp or status. Those live beside the graph (`atlas::flow::FlowIndex`), keyed by `node_key`.

## Step 3 — Scaffold the cloud collection layer

Create `atlas-lib/src/cloud/<provider>/`:
- `mod.rs` — re-exports `provider` and the resource modules; a good home for the provider's own HTTP client if it needs one (see `cloud/cloudflare/mod.rs`). Any such client needs a `with_base_url(token, url)` DI constructor so wiremock tests can point at a mock server.
- `provider.rs` — `pub async fn build_<provider>(verbose: bool, opts: &Settings) -> ProviderScan`.
- One file per resource type (e.g., `instance.rs`, `network.rs`), each exposing a runner that returns `Result<T, Box<dyn std::error::Error>>`.

**`build_*` is infallible — it always returns a `ProviderScan { provider, report }`.** There is no second failure channel. A provider that dies at the credential step still returns a scan whose empty collection is explained by a `report.record(SOURCE, FailureKind::Unauthorized, "credentials", e)`; a partial read returns what it got plus one failure per unreachable scope. Diagnose `Unauthorized` *before* fanning out, where the error is still typed — a boxed error later can only be recorded as the conservative `Unavailable`.

Fan out the per-scope collectors with the `collectors!` macro + `cloud::collector::run_all` (copy `amazon/provider.rs` or `google/provider.rs`), never a hand-rolled `tokio::join!` with a positional destructure. Each line carries the collection variant *and* the call, which makes the registration type-checked.

For HTTP-based APIs, `atlas-lib/src/api/google/client.rs` (paginated, OAuth) and `atlas-lib/src/cloud/cloudflare/mod.rs` (token header, `{success,result}` envelope) are the reference patterns. Never terminate a page loop on "the page came back short" — a clamped `per_page` makes that an ordinary response, and stopping there truncates the collection into what the differ reads as mass deletion.

## Step 4 — Add the provider variant to `cloud/definition.rs`

In `atlas-lib/src/cloud/definition.rs`:
- Add a `Provider::<Name>(..)` arm to the `Provider` enum, carrying the provider's collection type — per-scope (`Vec<(String, XCollection)>` like AWS/GCP) or flat (`Box<XCollection>` like Cloudflare).
- Add the `<Name>Collection` enum (one variant per collector's payload). Multi-field payloads need a *named* struct, not an inline struct variant, since only a path can be applied as a function by `collectors!`.

## Step 5 — Add CLI settings

In `atlas-lib/src/lib.rs` add the field to `Settings`, and in **both** `atlas-cli/src/main.rs` and `atlas-server/src/main.rs` add the matching clap flag (e.g. `--<provider>`, `--<provider>-projects`) and wire it through. The two binaries share the same provider flags on purpose — a provider that only works in the CLI cannot be part of the live twin.

## Step 6 — Create the projector

Create `atlas-lib/src/atlas/projector/<provider>.rs`:
- Implement `pub fn <provider>_projector(builder: &mut GraphBuilder, data: &..)`. When the payload is a slice of per-scope collections, the body is one call to `project_parallel(builder, data, |local, item| project_<provider>_collection(local, item))` — it fans the per-item projection out across rayon into its own `GraphBuilder` and merges the results back in order, so never hand-roll that.
- For each resource, link it in with `builder.link_to(parent, Node::<Type>(id.into()), Edge::Contains)` / `link_from(..)`. Use `get_or_add_node` / `get_or_add_ref` only when you need the `NodeIndex` for more than one edge, and `project_leaf!` (in `projector/mod.rs`, imported with `use super::project_leaf;`) for resources that only add a node.
- For any resource that exposes an IP or hostname, add `Node::GenericIpAddress` / `Node::GenericHostname` nodes connected via `Edge::RoutesTo` or `Edge::ResolvesTo`. These dedup automatically and are what stitch the new provider into the other clouds' estates.

## Step 7 — Register the projector

In `atlas-lib/src/atlas/projector/mod.rs`, add `pub mod <provider>;` and a new arm to `build`'s match on `CloudProvider` — the match is exhaustive, so the compiler demands it.

## Step 8 — Call `build_<provider>` from the engine

In `atlas-lib/src/atlas/engine.rs::collect`, add a `<provider>_future` gated on the relevant `Settings` field, add it to the `tokio::join!`, and add its result to the array that is flattened into the projector + report merge loop. Do not filter the scan out when the provider collected nothing — the report is how an empty result stays distinguishable from a failed one.

## Step 9 — Add fixtures and tests

- **Collector tests** (HTTP → struct, no credentials): add `atlas-lib/tests/<provider>_collectors.rs` using `wiremock` against the client's `with_base_url` seam. Pair a Layer-1 contract test (deserialize a realistic body) with a Layer-2 test exercising the real pagination and error paths. **Assert the specific fields the projector reads are populated** — our models are all `Option<T>` and serde ignores unknown fields, so a mismatched struct parses into all-`None` and passes a weak "did it parse?" check.
- **Fixtures**: in `atlas-lib/src/fixtures.rs`, add `pub fn <provider>() -> Provider` that populates **every** collection variant of the new provider with at least one resource, register it in `all()`, and give IP/hostname values that match another cloud's fixture so cross-cloud merging is exercised. Generic-node identity is byte-exact — the strings must match exactly.
- **Projection tests**: in `atlas-lib/src/atlas/tests.rs`, add a `<provider>_projection` test with semantic `assert_edge` / `assert_has_node` assertions, and extend `multi_cloud_seams_merge` for the new seams.
- The `every_node_kind_appears_in_fixture_graph` guard test will fail until every new node kind is actually produced.

## Step 10 — Verify

```bash
cargo build
cargo nextest run --all-targets   # includes the every_*_kind exhaustiveness guards
cargo run --example demo          # coverage table must show the new kinds, exit 0
cargo clippy
cargo xtask test                  # full gate across both workspaces + frontend
```

## Not in scope by default

Tier-1 event streams and Tier-2 flow logs are separate builds on top of this
(`atlas::event` / `atlas::flow`, see `docs/change_monitoring_design.md` Phase 5).
A new provider starts on Tier-3 polling alone, which is correct — just slower.
