---
name: add-provider
description: Scaffold a brand new cloud provider in cloud-atlas — cloud/ directory, projector, AtlasEngine integration, CLI flag, and fake-env test. Ask which provider if not specified.
disable-model-invocation: false
---

Add a new cloud provider. Ask for its name and at least one initial resource type if not given. Read `CLAUDE.md` first; its architecture rules are assumed. A provider is a new `CollectionSource`, the unit of retention, carry-forward and health reporting.

## 1. Collection source — `atlas-lib/src/atlas/collection.rs`

Add `CollectionSource::<Name>` and its `Display` arm.

## 2. Nodes — `atlas-lib/src/atlas/definition.rs`

- Add a `Some(CollectionSource::<Name>) => [..]` group to `nodes!`, one `<Provider><Type>(id) => "Provider::SubType({id})"` line per resource. That generates the variants, `Display`, `kind()`, `ALL_KINDS` and `owner()`.
- Reuse existing edges (`Contains`, `ConnectsTo`, `DependsOn`, `AttachedTo`, `HasIp`, `RoutesTo`, `ResolvesTo`). `TrafficFlow`, `Covers` and `Serves` are produced by the overlay and derivation passes, never by a projector.
- Identity only — no counters, timestamps or status.

## 3. Collection layer — `atlas-lib/src/cloud/<provider>/`

- `mod.rs`: re-exports, and the provider's HTTP client if any, with a `with_base_url(token, url)` constructor for wiremock.
- `provider.rs`: `pub async fn build_<provider>(verbose: bool, opts: &Settings) -> ProviderScan`.
- One file per resource type, each returning `Result<T, Box<dyn std::error::Error>>`.

**`build_*` is infallible.** A credential failure returns an empty scan with `report.record(SOURCE, FailureKind::Unauthorized, "credentials", e)`; a partial read returns what it got plus one failure per unreachable scope. Diagnose `Unauthorized` before fan-out, while the error is still typed.

Fan out with a `collectors!` macro + `cloud::collector::run_all` (copy `amazon/provider.rs` or `google/provider.rs`). Reference clients: `api/google/client.rs`, `cloud/cloudflare/mod.rs`. Never stop paging on a short page.

## 4. Provider types — `atlas-lib/src/cloud/definition.rs`

- Add a `Provider::<Name>(..)` arm: per-scope `Vec<(String, XCollection)>` (AWS/GCP) or flat `Box<XCollection>` (Cloudflare).
- Add `<Name>Collection`, one variant per payload; multi-field payloads need named structs.

## 5. Flags

Add the field to `Settings` in `atlas-lib/src/lib.rs`, and the clap flag to **both** `atlas-cli/src/main.rs` and `atlas-server/src/main.rs`.

## 6. Projector — `atlas-lib/src/atlas/projector/<provider>.rs`

- `pub fn <provider>_projector(builder: &mut GraphBuilder, data: &..)`. For per-scope data the body is `project_parallel(builder, data, |local, item| ..)`.
- Link with `link_to`/`link_from`; `project_leaf!` for node-only resources.
- Addresses and hostnames via `Node::ip`/`Node::hostname`: `ConnectsTo` when held, `ResolvesTo` for DNS, `RoutesTo` for traffic/rules.
- Register in `projector/mod.rs`: `pub mod <provider>;` and an arm in `build`'s exhaustive match.

## 7. Engine — `atlas-lib/src/atlas/engine.rs::collect`

Add a future gated on the `Settings` field, include it in the `tokio::join!` and the flattened results. Don't drop a scan that collected nothing — its report explains why.

## 8. Tests

- **Collectors**: `atlas-lib/tests/<provider>_collectors.rs` with wiremock; a contract test plus pagination/error tests. Assert the fields the projector reads.
- **Fixtures**: `pub fn <provider>() -> Provider` in `fixtures.rs` covering every collection variant, registered in `all()`, with addresses/hostnames that match another cloud's.
- **Projection**: `<provider>_projection` in `atlas/tests.rs`; extend `multi_cloud_seams_merge`. `every_node_kind_appears_in_fixture_graph` fails until every kind is produced.

## 9. Verify

```bash
cargo nextest run --all-targets
cargo run --example demo
cargo clippy
cargo xtask test
```

Tier-1 events and Tier-2 flow logs (`atlas::event`, `atlas::flow`) are separate follow-ups; a new provider starts on polling alone.
