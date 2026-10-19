# CLAUDE.md

## Project

Cloud Atlas maintains a **continuous live property graph** of multi-cloud infrastructure: an in-memory digital twin synchronized by event streams, flow logs, and periodic scans. Write code for long-running daemons.

## Architecture Rules

1. **Strongly typed enums only.** The graph is `petgraph::Graph<Node, Edge>` (`atlas-lib/src/atlas/definition.rs`). Never represent resources with raw strings or hashmaps.
2. **ENI is the networking pivot:** `Instance -HasIp-> ENI -AttachedTo-> Subnet`. `Node::AwsEc2Eni` is keyed by its `eni-` ID (carried by `DescribeInstances`, Config items, and flow logs). The owning instance is the `HasIp` edge, never part of the key: ENIs reattach, instances are multi-homed, and managed ENIs (NAT, LB, RDS, VPC Lambda) have no instance. Instances with no interfaces get no ENI and no direct `Instance -> Subnet` edge.
3. **`Display` on every type** in `Type::SubType(id)` form for `.dot` export.
4. **Failure ≠ absence.** "Could not read" and "is gone" stay distinct through `CollectionReport`, `carry_forward`, and `Retention`. Never swallow collector errors with `.ok()` or `if let Ok(..)`.
5. **Cross-cloud stitching uses generic pivots** (`GenericIpAddress`, `GenericPrivateIpAddress`, `GenericHostname`), built only via `Node::ip` / `Node::public_ip` / `Node::hostname` (`GraphBuilder` deduplicates via `HashMap<Node, NodeIndex>`).
   - **Private addresses are network-scoped:** RFC 1918, RFC 6598, link-local, loopback, and ULA values (`util::is_private`) build `GenericPrivateIpAddress { network, address }`. Only public addresses stitch directly across networks; private addresses resolve across networks only through modelled joins at derive time (`docs/network_connectivity_design.md`).
   - **Edge kinds are load-bearing:** `RoutesTo` = traffic/rules, `ResolvesTo` = DNS, `ConnectsTo` = resource *holds* the address (used by `derive::service` for ownership). DNS always projects `GenericHostname(name) -ResolvesTo-> <record> -ResolvesTo-> target`.
   - **Every projected edge needs an owned endpoint:** `carry_forward` scopes edges by owner, so an edge between two ownerless pivots would survive any outage (`every_projected_edge_has_an_owned_endpoint`).
6. **`Node` and `Edge` carry identity, never mutable state.** `Hash + Eq` is identity for `GraphBuilder` dedup, `patch::diff`, and `node_key`/`edge_key`. Changing fields (counters, `last_seen`, status) live beside the graph in `atlas::flow::FlowIndex` and derived observations, keeping `Edge::TrafficFlow` and `Edge::Serves` payload-free.

## Testing Without Cloud Credentials

Never call live cloud APIs in tests. Projectors test against the "Globex" estate (`atlas-lib/src/fixtures.rs`), which populates every provider collection and cross-cloud seam.

- `cargo nextest run` enforces exhaustiveness guards: every `Node` and `Edge` kind must appear in the fixture graph.
- `cargo run --example demo` projects fixtures + Tier-2 overlay (`fixtures::build_graph()`, combining `fixtures::topology()` and `fixtures::observed()`), writes `multi_cloud_demo.{dot,json}`, prints kind coverage, flows, and derived `Serves` edges, and exits non-zero if any kind is missing or the overlay is empty.
- Adding a resource type requires one `nodes!` line (`Variant(id) => "Provider::SubType({id})"`, or struct fields in the label—generating variant, `Display`, `kind()`, `ALL_KINDS`, and `owner()` under the authoritative `CollectionSource`), projector mapping, and fixture data. For AWS, also add its Config type to `TYPED_CONFIG_TYPES` (`projector/aws.rs`) with the item field an event names it by (or `None`): the Config catch-all never draws a listed type, and `every_aws_kind_config_records_has_one_source` fails until a new AWS kind is listed.

### Collector Tests (HTTP → Struct)

All model fields are `Option<T>` and serde ignores unknown fields, so a drifted schema parses as all-`None`. **Always assert that fields read by the projector are populated.**

- **reqwest clients** (`GoogleApiClient`, `CloudflareApiClient`, `AzureApiClient`): `wiremock` + `with_base_url(token, url)` in `atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs`. Pair a realistic contract test with pagination/error tests (GCP `nextPageToken`, Azure `$skipToken`, Cloudflare `{success,result}`).
- **`cloudflare` crate** (zone/dns/kv): `Environment::Custom(format!("{}/client/v4/", server.uri()))` via `serve_crate`/`crate_client`.
- **AWS SDK:** `StaticReplayClient` via `.http_client(...)` on a test `SdkConfig` in unit module `cloud/amazon/collector_tests.rs` (`replay_config` feeds responses in order; unit test because `aws-config` is a normal dependency).
- **Azure mapping:** `provider::map_resources` returns `(collections, CollectionReport)` without failing the batch; undeserializable ARG rows are skipped and reported as `Malformed` (capped at `MAX_REPORTED_ROWS` + tail count).

## Build, Run & Dev Orchestration

```bash
# CLI (writes gitignored atlas.dot and atlas.json)
cargo build --release                              # target/release/atlas
cargo run --bin atlas -- --regions us-east-1 us-west-2
cargo run --bin atlas -- --gcp-projects my-project
cargo run --bin atlas -- --cloudflare              # requires CLOUDFLARE_API_TOKEN
cargo run --bin atlas -- --azure-subscriptions sub-id
cargo run --bin atlas -- --daemon --verbose        # re-scans every 60s

# Orchestration (prefer over manual commands)
cargo xtask dev --demo      # wasm (if stale) → atlas-server :4681 → frontend :4680
cargo xtask dev             # real collection; forwards provider flags + --demo only
cargo xtask wasm [--force]  # rebuild atlas-web/static/pkg/ (required after SNAPSHOT_VERSION bump)
cargo xtask demo            # regenerate multi_cloud_demo.json and atlas-web/static/snapshot.json
cargo xtask test [--e2e]    # nextest (root) → nextest (atlas-render) → biome → svelte-check → bun unit [→ playwright]
```

**Test runner:** `cargo nextest run --all-targets` (falls back to `cargo test --all-targets`; `--all-targets` compile-checks `examples/demo.rs` and `layout_demo.rs`; add `cargo test --doc` if doctests are introduced). Configured in `.config/nextest.toml` (`default`: fail-fast, 30 s slow warning; `-P ci`: full run, 1 retry).

## Live Server (`atlas-server/`)

`atlas-server` owns a persistent graph, reconciles on an interval (Tier 3 via `AtlasEngine::collect`), diffs (`patch::diff`), and broadcasts `GraphPatch`es over WebSocket without ever wiping the graph.

```bash
cargo run -p atlas-server -- --demo
cargo run -p atlas-server -- --regions us-east-1 --poll-secs 30 --port 8080 --retain-scans 3
cargo run -p atlas-server -- --aws-event-queue <sqs-url> --aws-flow-log-queue <sqs-url> --flow-ttl-secs 300
```

### Collection Outcomes & Retention

- **`AtlasEngine::collect`** returns `Scan { builder, report }`. `build_*` is infallible, returning `ProviderScan { provider, report }`.
- **Per-source carry-forward:** `poll::reconcile` (and CLI `AtlasEngine::install`) folds the prior graph forward for **unreadable sources only** (`patch::carry_forward` + `report.unreadable_sources()`) via `Node::owner()`, keeping healthy providers authoritative for deletions. Ownerless pivots (`Generic*`, `ExternalService`) are retained during any incomplete scan.
- **Retention budget (`patch::Retention`):** Unreadable sources are held for `--retain-scans` consecutive scans (default 10) before release; recovery resets the streak. Classified once while errors are still typed (`collection::FailureKind`):
  - `Unavailable` (throttling, timeouts, 5xx, boxed errors in `run_all`): full `--retain-scans` budget.
  - `Unauthorized` (bad/missing credentials, probed pre-fan-out in GCP `authenticate()`, Cloudflare `clients()`, `AzureApiClient::new()`, and per-region `amazon::resolve_credentials`): `Retention::AUTH_BUDGET` (2), capped by `--retain-scans`. Applies only when *every* failure for that source was `Unauthorized` (`report.unreadable_kind`).
  - `Malformed` (drifted row in a readable response): **never enters `unreadable_sources()`** and holds nothing (`is_complete()` is false, but removals remain authoritative).

### Demo Mode (`atlas-server/src/demo.rs`)

All credential-free behaviour (canary DNS parity toggle, 1-in-4 burst flow, 5 s poll, flow TTL, seed graph) lives in `demo.rs`; fixture data (`flows`, `burst_flow`, `canary`) lives in `atlas-lib/src/fixtures.rs`. Tracked in `docs/demo_design.md`.

- **No demo leakage into production defaults:** `Source::default_poll` / `default_flow_ttl` return 60 s and `FlowIndex::DEFAULT_TTL` (15 min) for `Live`, and `demo::POLL` / `demo::flow_ttl(poll)` for `Demo`.
- **Projector shapes only:** Demo mutations run through projectors (`the_live_change_is_a_shape_a_scan_produces`).
- **Flows via `FlowIndex`:** `demo::graph` returns `fixtures::topology()`, and `Source::seed_flows` primes `FlowIndex` (`the_scan_graph_carries_no_traffic_of_its_own`).

### Tier 1: Live Event Feed (`--aws-event-queue`)

`atlas::event::ChangeEvent` + `EventApplier`; `cloud/amazon/events.rs` drains EventBridge-fed SQS (Config items, EC2 state changes, CloudTrail). `poll::run` `select!`s over reconcile and both feeds under one writer.

1. **Projector parity:** Adapters must emit only what the full-scan projector emits (`project_instance`/`InstanceFacts`, `project_interface`/`InterfaceFacts`, `use_aws_resource`/`use_global`). A Config item for a type in `TYPED_CONFIG_TYPES` becomes only its typed node, never also an `AwsConfigResource`. Types with no event key (keyed differently, like Route 53's `/hostedzone/` prefix and SQS URLs, or filtered by state the item lacks, like peerings and transit gateways) and deferred `link_interface_owners` edges are omitted (`an_instance_event_produces_only_what_a_full_scan_would`, `an_interface_event_produces_only_what_a_full_scan_would`).
2. **Additive only:** Creates/modifies never remove unmentioned edges; deletes remove only the named node.
3. **Last-writer-wins on cloud record time** per resource (bounded table; never substitute arrival time for unparseable timestamps).
4. **Separate health:** `AppState::stream_report` (`/collection.json` → `stream`) never enters `unreadable_sources()`. Unparseable messages are `Malformed`, reported, and deleted.

### Tier 2: Liveness Overlay (`--aws-flow-log-queue`)

`atlas::flow::FlowObservation` + bounded, expiring `FlowIndex` (`AppState::flows`); `cloud/amazon/flow_logs.rs` drains S3 event notifications from SQS, gunzips, and parses flow logs.

1. **Metrics beside the graph:** Flow edges carry `FlowStats` (`last_seen`, `packets`, `bytes`); nodes carry `Liveness` (`last_seen`, verdict, no volume to avoid multi-counting across up to 4 keys per record).
2. **Ownerless pivots only:** Never invents typed nodes; creates only generic IP pivots.
3. **Expiry via Tier 3:** `poll::reconcile` folds `FlowIndex::overlay` into each scan before diffing; `patch::carry_forward` never holds `TrafficFlow`.
4. **Separate health:** `AppState::flow_report` (`/collection.json` → `flows`) never enters `unreadable_sources()`.
5. **Parsing & scoping:** Reads each object's header line (falling back to v2 default); missing `action` → `None` (`observed`). `interface-id` and `instance-id` are read independently. Private IPs take the record's `vpc-id` column, else the `interface-id`'s network in `InterfaceNetworks` (snapshotted by `poll::run`); private records with neither are dropped and reported as `Malformed`.

### Frontend Overlay & API

- **Overlay rendering:** Flow edges use verdict colour and log-scaled `packets` width; active nodes show a pulsing halo; packets animate on `canvas.traffic-layer` (excluded from the settled-graph stability test, verified by its own animation test). `Edge::Serves` styles by `inferred`/`confirmed` status (`SERVES_STATUS_COLORS`, `servesColor`/`servesSize` in `style.ts`) and never animates beads (`FLOW_KIND` filter). `GraphController::setServiceView` hides all non-`Serves` edges and untouched nodes via Sigma reducers.
- **Endpoints:** `GET /snapshot.json` (v3: `nodes`, `edges`, `observations`), `GET /collection.json` (`complete`, `unreadable`, `failures`, `stream`, `flows`), `GET /ws` (server pushes `snapshot`/`patch`; client sends `subscribe`, `get_snapshot`, `get_neighbors`). Frontend (`bun dev` on `:4680`) connects to `ws://<host>:4681/ws` (override with `?server=ws://…` or offline `?static`).

## Rendering Workspace (`atlas-render/`)

Separate Cargo workspace (`exclude`d from root) that must never depend on `atlas-lib`. Contract is versioned JSON (`SNAPSHOT_VERSION = 3`, pinned in `atlas-lib/src/atlas/export.rs`, `atlas-render/atlas-layout/src/graph.rs`, and `atlas-render/atlas-web/src/lib/graph.ts`—bump all three and run `cargo xtask wasm`).

- `atlas-layout`: deterministic ForceAtlas2 (Barnes-Hut, flat `f32` buffer, optional `parallel` rayon feature).
- `atlas-layout-wasm`: `wasm-bindgen` bridge.
- `atlas-web`: SvelteKit + Sigma.js Bun app (`bun install && bun run wasm && bun dev`).
- Test inside `atlas-render/` with `cargo nextest run`.

## Auth & Toolchain

- **Auth (unavailable locally):** AWS standard credential chain; GCP OAuth2 (`yup-oauth2`); Cloudflare `CLOUDFLARE_API_TOKEN`; Azure `az login` (`AzureCliCredential`).
- **Toolchain:** Rust **2024 edition** (let-chains), stable ≥ 1.85. VCS is **jj** on git (`jj describe` → `jj new` → `jj squash` → `jj git push --change <id>`).

## Established Helpers

- **Fan-out (`cloud::collector::run_all`):** Runs `Vec<NamedCollector<'_, T>>` concurrently, reporting `{scope}/{name}` failures. AWS and GCP use a local `collectors!` macro (`"name" => Collection::Variant, call(..)`); multi-field payloads use named structs (`AWSLoadBalancing`, `AWSRoute53`, `AWSNetworking` in `cloud/definition.rs`).
- **Provider clients:**
  - **AWS:** `cloud/amazon.rs::load_config(region)` once per region; register in `amazon/provider.rs`. Page with the SDK's `into_paginator().items()`; an operation without one (Route 53 record sets, EventBridge buses) loops by hand and fails on a cursor that does not advance.
  - **GCP:** `GoogleApiClient::paginated_list` (`api/google/client.rs`); register in `google/provider.rs`.
  - **Azure:** `AzureApiClient::query_graph` errors on missing/non-array `data` or `totalRecords > rows.len()`. `azure_types!` in `azure/provider.rs` drives the ARG filter and exhaustive `map_resources` dispatch (`leaf!` for `{id, name, location}`).
  - **Cloudflare:** `CloudflareApiClient::get` / `get_paged` (cursor endpoints like R2) and `cloudflare::paginate(per_page, |page| ..)`. **Never stop paging on a short page.**
- **Projectors:** `projector::project_parallel` (per-scope rayon fan-out + ordered merge); `project_leaf!` for leaf nodes; `GraphBuilder::link_to` / `link_from` (use `get_or_add_node`/`get_or_add_ref` only when reusing the index).
- **Graph mutation:** `add_edge` deduplicates; `GraphBuilder::merge(&graph)` folds by node identity; `GraphBuilder::remove_node` repairs `node_map` after petgraph swap-remove (never call `graph.remove_node` directly); `patch::merge_additions` merges a subgraph and returns only newly added elements.
- **Pivots (`Node::ip(value, network)`, `Node::public_ip(value)`, `Node::hostname(value)`):** Canonicalise via `util::canonical_address` / `canonical_hostname`. `network` equals the owning network node's ID (`Node::network_scope()`): AWS VPC ID, lowercased Azure VNet ID (from subnet), or GCP `projects/P/global/networks/N` (`projector/gcp.rs::canonical_network`). Azure NSG rules use each attached subnet's VNet. When no network exists (DNS records, unattached NSGs, non-VPC resources), the naming resource's ID stands in as `network`.
- **AWS interfaces & `scanned_instances` gate:** `DescribeNetworkInterfaces` projects all ENIs and their private IPs (`project_instance` falls back to `facts.private_ip` only when an instance has no ENIs) and links EIPs (`Eni -HasIp-> Eip` via `network_interface_id`). `DescribeInstances` filters to running/pending, so post-parallel passes (`link_interface_owners` and `link_targets`) gate instance references on `scanned_instances`. `link_targets` links `instance` targets via `ConnectsTo` and `ip` targets via `RoutesTo` a VPC pivot, consulting `derive::held_addresses` to skip IPs held (locally or across a join) by an ENI attached to an unscanned instance (`a_stopped_ip_target_is_not_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred`).
- **Derived edges (`atlas::derive::all(&mut builder, &flows)`):** Single entry point called after `carry_forward` and `FlowIndex::overlay`. Stateless, idempotent, and excluded from `Edge::is_projected()` (`TrafficFlow`, `Covers`, `Serves`).
  - `derive::containment`: links host IPs into covering CIDR pivots (`Edge::Covers`) within the same network.
  - `derive::service`: emits `Edge::Serves` in traffic direction (`inferred` from `CHAINS` over projected edges, `confirmed` from active `FlowIndex` records, oriented by `flow::orientation` port heuristics and `FlowStats` majority). `address_owners` walks `ConnectsTo` + transitive `HasIp` and resolves unheld private pivots across single unambiguous network joins (`JOINS` in `derive/networks.rs`: `Through` peering/TGW hubs or `Direct` VNet/GCP peerings, non-transitive). Snapshots emit `derive::observations`; patches emit `derive::DerivedObservations::changed`.

Historical findings and decisions live in [`docs/audit_findings.md`](docs/audit_findings.md).
