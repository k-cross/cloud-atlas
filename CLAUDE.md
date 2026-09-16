# CLAUDE.md

## Project

Cloud Atlas builds a **continuous live property graph** of multi-cloud infrastructure: an in-memory digital twin kept in sync by event streams, not a point-in-time snapshot. Write code for long-running daemons.

## Architecture Rules

1. **Strongly typed enums only.** The graph is `petgraph::Graph<Node, Edge>`; both enums live in `atlas-lib/src/atlas/definition.rs`. Never represent resources with raw strings or hashmaps.
2. **ENI is the networking pivot:** `Instance -HasIp-> ENI -AttachedTo-> Subnet`. `Node::AwsEc2Eni` is keyed by its own `eni-` id — the id `DescribeInstances`, Config items and flow logs all carry. The owning instance is the `HasIp` edge, never part of the key: ENIs get reattached, instances are multi-homed, and most ENIs (NAT, LB, RDS, in-VPC Lambda) have no instance. An instance that reports no interfaces gets no ENI and no subnet path — never a direct `Instance -> Subnet` edge.
3. **`Display` on every type**, in the `Type::SubType(id)` form, for clean `.dot` output.
4. **A failure must never look like an absence.** "Could not read" and "is gone" stay distinct all the way to the differ — see `CollectionReport` under Live Server.
5. **Cross-cloud stitching goes through generic pivots** (`GenericIpAddress`, `GenericHostname`), built only via `Node::ip`/`Node::hostname` (see Helpers). Edge kind is load-bearing: `RoutesTo` = traffic, `ResolvesTo` = DNS, `ConnectsTo` = the resource *holds* this address. `derive::service` treats `ConnectsTo` as ownership, so DNS producers must use `ResolvesTo`. `GraphBuilder` dedups identical pivots across clouds via its `HashMap<Node, NodeIndex>`.
6. **`Node` and `Edge` carry identity, never mutable state.** Their `Hash + Eq` *is* identity: `GraphBuilder` dedups on it, `patch::diff` compares on it, `node_key`/`edge_key` derive the wire id from it. A changing field (counter, `last_seen`, status) would dodge dedup and turn every diff into remove-then-add. Such data lives beside the graph keyed by the stable key — `atlas::flow::FlowIndex`, which is why `Edge::TrafficFlow` and `Edge::Serves` are payload-free.

## Testing Without Cloud Credentials

No live credentials exist locally; never write tests that call real cloud APIs. Projection tests run against the fake "Globex" estate in `atlas-lib/src/fixtures.rs`, which populates every collection variant of every provider plus deliberate cross-cloud seams.

- `cargo nextest run` includes exhaustiveness guards: every `Node`/`Edge` kind must appear in the fixture graph.
- `cargo run --example demo` projects the fixtures, folds in the Tier-2 overlay, writes `multi_cloud_demo.{dot,json}`, prints kind coverage, observed flows and derived service edges, and exits non-zero if any kind is missing or the overlay is empty. `fixtures::build_graph()` is topology + overlay; `fixtures::topology()` and `fixtures::observed()` are each half.

Adding a resource type = one `nodes!` line + projector mapping + fixture data; the guards enforce all three. The `nodes!` line (`Variant(id) => "Provider::SubType({id})"`; struct variants name every field in the label) generates the variant, `Display`, `kind()`, `ALL_KINDS` and `owner()`. File it under the `CollectionSource` whose scan is authoritative for it — that grouping scopes carry-forward on an incomplete scan.

### Collector tests (HTTP → struct)

Fixtures test projectors; collectors are tested by replaying canned responses:

- **reqwest clients** (`GoogleApiClient`, `CloudflareApiClient`, `AzureApiClient`): `wiremock` + each client's `with_base_url(token, url)` constructor. Tests in `atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs` pair a contract test (realistic body) with a pagination/error test (GCP `nextPageToken`, Azure `$skipToken`, Cloudflare's `{success,result}` envelope).
- **`cloudflare` crate** collectors (zone/dns/kv): seam is `Environment::Custom(format!("{}/client/v4/", server.uri()))` (`serve_crate`/`crate_client` helpers). The crate's structs are strict, so a drifted body fails outright.
- **AWS SDK**: `StaticReplayClient` via `.http_client(...)` on a test `SdkConfig`, which exercises the real SDK deserializer. All AWS collectors are in the unit module `cloud/amazon/collector_tests.rs` (`replay_config` feeds one response per request, in order); it can't be an integration test because `aws-config` is a normal dep.
- **Azure mapping**: `provider::map_resources` is split from the fetch and returns `(collections, CollectionReport)` without failing — an undeserializable row is skipped and reported (capped at `MAX_REPORTED_ROWS` plus a tail count), since ARG returns the whole tenant in one response.

**Our models are all `Option<T>` and serde ignores unknown fields**, so a mismatched struct parses as all-`None`. Assert the fields the projector reads are populated, not just that parsing succeeded.

## Build & Run

The root is a virtual workspace with several binaries, so name one:

```bash
cargo build --release                              # target/release/atlas
cargo run --bin atlas -- --regions us-east-1 us-west-2
cargo run --bin atlas -- --gcp-projects my-project
cargo run --bin atlas -- --cloudflare              # needs CLOUDFLARE_API_TOKEN
cargo run --bin atlas -- --azure-subscriptions sub-id
cargo run --bin atlas -- --daemon                  # re-scans every 60s
cargo run --bin atlas -- --verbose
```

Writes `atlas.dot` and render snapshot `atlas.json` (both gitignored) to the working directory.

## Dev Orchestration (`cargo xtask`)

Prefer these over running pieces by hand:

```bash
cargo xtask dev --demo    # credential-free: wasm (if stale) → atlas-server :4681 → frontend :4680; Ctrl-C stops all
cargo xtask dev           # real collection; pass provider flags
cargo xtask wasm [--force]  # rebuild atlas-web/static/pkg/ if atlas-layout is newer (always after a SNAPSHOT_VERSION bump)
cargo xtask demo          # regenerate multi_cloud_demo.json
cargo xtask test [--e2e]  # nextest (root) → nextest (atlas-render) → biome → svelte-check → bun unit [→ playwright]
```

`xtask dev` forwards only provider flags and `--demo`; run `atlas-server` directly for `--aws-event-queue`, `--aws-flow-log-queue`, `--retain-scans` or `--flow-ttl-secs`.

**nextest is the test runner** (`cargo nextest run --all-targets`, falling back to `cargo test --all-targets` if not installed). `--all-targets` matters: nextest skips `examples/` otherwise, and that is what compile-checks `demo.rs` and `layout_demo.rs`. nextest never runs doctests (there are none; add `cargo test --doc` if that changes). Config is `.config/nextest.toml` in each workspace: `default` is fail-fast with a 30s slow warning, `ci` (`-P ci`) runs everything with one retry.

## Live Server (`atlas-server/`)

`atlas-cli` is the one-shot path. `atlas-server` owns a persistent graph, reconciles it against providers on an interval (Tier 3, via `AtlasEngine::collect`), diffs each scan (`patch::diff`), and pushes `GraphPatch`es over WebSocket. It never wipes the graph.

```bash
cargo run -p atlas-server -- --demo                  # Globex fixtures, port 4681
cargo run -p atlas-server -- --regions us-east-1     # same provider flags as the CLI
cargo run -p atlas-server -- --poll-secs 30 --port 8080
cargo run -p atlas-server -- --retain-scans 3
cargo run -p atlas-server -- --aws-event-queue <sqs-url>      # Tier 1
cargo run -p atlas-server -- --aws-flow-log-queue <sqs-url>   # Tier 2
cargo run -p atlas-server -- --flow-ttl-secs 300     # unset: the source chooses
```

**Collection outcomes are part of the contract.** `AtlasEngine::collect` returns `Scan { builder, report }`; the `CollectionReport` (`atlas::collection`) names every source that could not be read. A failed fetch must never reach the projector as an empty collection, or the differ broadcasts a removal for everything that source owns. `poll::reconcile` folds the live graph forward for **unreadable sources only** (`patch::carry_forward` + `report.unreadable_sources()`), so a failed provider is additive-only while healthy providers stay authoritative — otherwise one permanently failing collector would block deletions everywhere. Ownership is `Node::owner()`; ownerless pivots (`GenericIpAddress`/`GenericHostname`/`ExternalService`) are retained on any incomplete scan. The CLI daemon applies the same policy in `AtlasEngine::install`.

**Retention is a budget** (`patch::Retention`). A source is held for `--retain-scans` consecutive incomplete scans (default 10) and then released so the differ can delete what could not be confirmed; recovery resets the streak. The CLI daemon uses `Retention::default()`. The budget depends on `collection::FailureKind`, classified once where the error is still typed — never parsed from a message:

- `Unavailable` — throttling, timeouts, 5xx. Full budget. `run_all` records this, since boxed errors have lost their kind.
- `Unauthorized` — bad/missing credentials. `Retention::AUTH_BUDGET` (2), capped by `--retain-scans`. Diagnosed before fan-out: GCP `authenticate()`, Cloudflare `clients()`, `AzureApiClient::new()`, and `amazon::resolve_credentials` (probed once per region, since the SDK resolves credentials lazily).
- `Malformed` — a drifted row in a response that was read. **Not unreadable**: never enters `unreadable_sources()`, holds nothing. Otherwise one bad ARG row would freeze deletions across all of Azure.

A source is `Unauthorized` only if *every* failure was a refusal (`report.unreadable_kind`); mixed evidence gets the longer budget. `is_complete()` is stricter than "authoritative": a `Malformed`-only scan is incomplete but trusted for removals. Retention is per provider, so a long outage releases that provider's whole estate at once; finer retention would need per-node provenance (an `AwsEc2Vpc` comes from five collectors).

**`build_*` is infallible** and returns `ProviderScan { provider, report }`. A provider that fails at credentials returns an empty scan explained by its report (`"credentials"`, `"auth"`); a partial read returns what it got plus one failure per unreachable scope. Never swallow a collector error with `.ok()` or `if let Ok(..)` — record it.

### Demo mode lives in `atlas-server/src/demo.rs`

All credential-free behaviour — the sentinel flipping by tick parity, the burst-flow cadence (one tick in four), the seed graph, the demo flow TTL — is in `demo.rs`. `poll::Source` only picks `Live` or `Demo` and delegates; `main.rs` has no demo constants.

**A demo detail must never become a production default.** `--flow-ttl-secs` is unset-able, and `Source::default_flow_ttl` asks the source: live answers `FlowIndex::DEFAULT_TTL` (15 min), demo answers `demo::flow_ttl(poll)`. Fixture *data* (`fixtures::flows`, `fixtures::burst_flow`) stays in `atlas-lib`; only the *behaviour* lives in `demo.rs`.

`demo::graph` returns `fixtures::topology()`, not `build_graph()`: traffic baked into a scan can never be removed by the differ or expired by a TTL. Demo flows reach the graph through `FlowIndex` like real ones, and `Source::seed_flows` primes the index so the first snapshot has them. Pinned by `the_scan_graph_carries_no_traffic_of_its_own`.

### Tier 1: live event feed (`--aws-event-queue`)

`atlas::event` is the normalized model (`ChangeEvent`: one resource, what happened, and the neighbourhood the payload described); `EventApplier` folds events in between scans. `cloud/amazon/events.rs` reads AWS Config items, EC2 state changes and CloudTrail management events off an EventBridge-fed SQS queue. `poll::run` `select!`s over the reconcile ticker and both live feeds, so all tiers share one writer. Rules:

1. **An adapter may only produce what the full-scan projector produces**, or reconciliation deletes it and the next event re-adds it forever. Instances go through `projector::aws::project_instance`/`InstanceFacts`, ENIs through `project_interface`/`InterfaceFacts`, and the Config catch-all reuses `use_aws_resource`/`use_global`. Types Config keys differently are left unmapped (Route 53 zones' `/hostedzone/` prefix, SQS URLs). ENI events omit both `link_interface_owners` edges: a balancer ARN would need a guessed partition, and an attachment needs the scan's running/pending instance list. Pinned by `an_instance_event_produces_only_what_a_full_scan_would` and `an_interface_event_produces_only_what_a_full_scan_would`.
2. **Events add; only Tier 3 garbage-collects.** A create/modify never removes an unmentioned edge; a delete removes only the named node. That is what makes an at-least-once, unordered feed safe.
3. **Last-writer-wins on the cloud's record time**, per resource, so a redelivered create can't resurrect a deleted resource. Never substitute arrival time for an unparseable timestamp. The ordering table is bounded.
4. **Stream health is not scan health.** Feed failures live in `AppState::stream_report` (`/collection.json` → `stream`) and never enter `unreadable_sources()`. An unparseable message is `Malformed`, reported and deleted.

Known, tested race: a reconciliation installs the estate as of scan *start*, so a resource created by an event mid-scan is removed that tick and re-added by the next. Adapter tests replay EventBridge bodies (`cloud/amazon/events/tests.rs`); the SQS path is covered end to end with `StaticReplayClient`.

### Tier 2: liveness overlay (`--aws-flow-log-queue`)

`atlas::flow`: `FlowObservation` is the normalized record, `FlowIndex` the bounded, expiring store beside the graph (`AppState::flows`). `cloud/amazon/flow_logs.rs` drains an SQS queue of S3 notifications for VPC Flow Log objects, gunzips and parses them. Rules:

1. **Metrics beside the graph** (rule 6). Flow edges get `FlowStats` (`last_seen`, `packets`, `bytes`); nodes get `Liveness` (`last_seen`, verdict) and no volume — one record names up to four keys, so per-node volume would multiply-count. `last_seen` composes as a max, so it's safe everywhere.
2. **Observations create only ownerless nodes.** A record has an IP and interface id, not a type, tags or subnet, so a typed endpoint the scan hasn't found is skipped, never invented. Generic pivots are the exception, which is how cross-cloud flows land as real edges.
3. **Expiry deletes via Tier 3.** `poll::reconcile` folds `FlowIndex::overlay` into the scanned graph before diffing; a quiet flow is simply absent and the differ removes it. Hence `patch::carry_forward` never holds `TrafficFlow`.
4. **Flow health is its own report** (`AppState::flow_report`, `/collection.json` → `flows`) and never enters `unreadable_sources()`.

Parsing reads each object's header line for field order, falling back to the v2 default only when absent — custom formats are chosen field by field. No `action` column → `action == None` (status `observed`). `interface-id` and `instance-id` are read independently, never derived from each other, and neither creates a node. Tests: `cloud/amazon/flow_logs/tests.rs` (objects and a full SQS+S3 replay); overlay policy in `atlas/flow.rs`.

### Frontend rendering of the overlay

Flow edges take their verdict colour and a log-scaled width from `packets`; nodes heard from get a pulsing halo; packets animate as beads on a separate `canvas.traffic-layer` (bead count and transit time log-scaled). That canvas is excluded from the "settled graph is still" e2e test, and a separate test asserts it moves.

`Edge::Serves` is styled by status (`SERVES_STATUS_COLORS`, `servesColor`/`servesSize` in `style.ts`): `confirmed` saturated, `inferred` receding. No observation (not yet arrived, or expired) → `inferred` styling. The traffic canvas keys off `FLOW_KIND`, so it never animates `Serves` (`service.test.ts`: "never animates packets along a derived edge").

**Service view** (`GraphController::setServiceView`) *hides* — not dims — every non-`Serves` edge and every node no `Serves` edge touches, via Sigma reducers. The traffic canvas already skips hidden endpoints.

### API

- `GET /snapshot.json` — snapshot v3: nodes, edges, `observations`.
- `GET /collection.json` — `complete`, `unreadable`, `failures` (source/scope + `kind`), `stream`, `flows` (+ observed count). The only way to tell "holds nothing" from "unreachable" from "dropped a row".
- `GET /ws` — bidirectional: server pushes `snapshot` then `patch`es; client may send `subscribe`, `get_snapshot`, `get_neighbors`.
- Frontend: `bun dev` in `atlas-render/atlas-web/` (:4680) connects to `ws://<host>:4681/ws`; override with `?server=ws://…`, force offline with `?static`.

## Rendering Workspace (`atlas-render/`)

A **separate cargo workspace**, `exclude`d from the root, that must never depend on `atlas-lib` (the SDK tree doesn't build for wasm). The only contract is the versioned snapshot JSON and the same-shaped `GraphPatch`. `SNAPSHOT_VERSION` (currently **3**) is pinned in `atlas-lib/src/atlas/export.rs`, `atlas-render/atlas-layout/src/graph.rs` and `atlas-render/atlas-web/src/lib/graph.ts`. On a shape change, bump all three and rebuild the wasm — the compiled engine rejects mismatched snapshots.

- `atlas-layout` — pure-Rust ForceAtlas2 (Barnes-Hut, deterministic, flat `f32` buffer); `parallel` feature uses rayon.
- `atlas-layout-wasm` — wasm-bindgen bridge (`cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown`).
- `atlas-web` — SvelteKit + Sigma.js, a **bun** app: `bun install && bun run wasm && bun dev`.
- Test with `cargo nextest run` **inside `atlas-render/`**; the root run doesn't cover it.
- Static e2e: `cargo run --example demo` (root) → `cargo run --example layout_demo -- ../multi_cloud_demo.json` (in `atlas-render/`) → `bun dev`, open `/?static`.

## Auth (reference only — unavailable locally)

| Provider | Mechanism |
|---|---|
| AWS | Standard credential chain |
| GCP | OAuth2 browser flow (`yup-oauth2`) |
| Cloudflare | `CLOUDFLARE_API_TOKEN` (hard error if missing) |
| Azure | `az login` (`AzureCliCredential`) |

## Toolchain & VCS

Rust **2024 edition** (let-chains), stable ≥ 1.85. VCS is **jj** on git: `jj describe` → `jj new` → `jj squash` → `jj git push --change <id>`.

## Established Helpers (use these, don't re-duplicate)

- **Fan-out**: `cloud::collector::run_all` takes a `Vec<NamedCollector<'_, T>>` and runs them concurrently, reporting failures as `{scope}/{name}`. AWS and GCP build the list with a local `collectors!` macro, one `"name" => Collection::Variant, call(..)` line each. The macro applies the variant, so a collector returns its natural type and a mismatched registration fails to compile. Multi-field payloads therefore need a named struct (`AWSLoadBalancing`, `AWSRoute53`, `AWSNetworking` in `cloud/definition.rs`).
- **AWS**: `cloud/amazon.rs::load_config(region)` once per region; collectors take `&SdkConfig`. Register in `amazon/provider.rs`'s `collectors!`.
- **GCP**: every list endpoint goes through `GoogleApiClient::paginated_list` (`api/google/client.rs`). Register in `google/provider.rs`'s `collectors!`.
- **Azure**: `AzureApiClient::query_graph` treats a missing/non-array `data` as an error, never an empty tenant, and errors when `totalRecords` exceeds rows returned (too few is data loss; too many is harmless). The `azure_types!` list in `azure/provider.rs` drives both the ARG `type in~` filter and `map_resources`' exhaustive dispatch; `leaf!` covers `{id, name, location}`.
- **Cloudflare**: `CloudflareApiClient::get` for raw REST (`get_paged` also returns `result_info` for cursor endpoints like R2); `cloudflare::paginate(per_page, |page| ..)` for page-numbered crate endpoints. **Never stop paging on a short page** — a clamped `per_page` makes that normal, and truncation reads as mass deletion.
- **Projectors**: `projector::project_parallel` is the per-scope rayon fan-out + ordered merge (AWS, GCP, Azure). `project_leaf!` (`use super::project_leaf;`) for node-only resources: `(builder, items, field, Node::Variant)` for serde models, `(builder, items, accessor(), Node::Variant, parent, edge)` for AWS SDK models. Link with `GraphBuilder::link_to(from, node, edge)` / `link_from(node, to, edge)`; use `get_or_add_node`/`get_or_add_ref` only when the index is needed for several edges.
- **Graph mutation**: `add_edge` dedups. `GraphBuilder::merge(&graph)` is the one node-identity fold (`patch::carry_forward` wraps it). `GraphBuilder::remove_node` reports the edges that died and repairs `node_map` after petgraph's swap-remove — never call `graph.remove_node` directly. `patch::merge_additions` folds a subgraph into the live graph and reports only what was new (measured before merging); both live tiers use it.
- **Event/flow adapters**: `EventApplier` is the only applier of `ChangeEvent`s (idempotency + ordering), and `FlowIndex` the only store of `FlowObservation`s (admission, expiry, capacity). Adapters build context with the projector's own functions.
- **Pivots**: `Node::ip(value)` / `Node::hostname(value)` are the only constructors for `GenericIpAddress`/`GenericHostname`. They canonicalise via `util::canonical_address` / `canonical_hostname`, since pivots match by exact value (`2001:0db8:…:0010` ≠ `2001:db8::10` otherwise). Unparseable values (prefix-list ids, service tags) pass through untouched.
- **AWS interfaces** (`cloud/amazon/network_interface.rs`, `DescribeNetworkInterfaces`) make every ENI a node. A private IP belongs to its interface; `project_instance` falls back to `facts.private_ip` only for an instance with no interfaces. Owner edges only where the description is unambiguous: NAT gateway in the projector arm, load balancer in `link_interface_owners` (needs the balancer collection to build the ARN). Lambda, VPC endpoint and RDS interfaces stay unowned. An Elastic IP is held by its interface (`Eni -HasIp-> Eip`, from `network_interface_id`), never by `instance_id`. Target-group targets by `target_type`: `ip` → `RoutesTo` a pivot (never `ConnectsTo`), Lambda/ALB skipped, `instance` linked in `link_instance_targets`.
- **`scanned_instances` gates every edge built from another collection's mention of an instance.** `DescribeInstances` is filtered to running/pending, but stopped instances stay attached to ENIs and registered in target groups, and `link_to` would invent a bare node the scan excluded. `link_interface_owners` and `link_instance_targets` run after `project_parallel` and take the set, computed once in `aws_projector`. New edges that name an instance by id belong there too.
- **Derived edges**: `atlas::derive::all(&mut builder, &flows)` is the **only** entry point — its passes are private modules, so the compiler rejects direct calls. Call it wherever a graph is finalized, after `carry_forward` and `FlowIndex::overlay` (`poll::reconcile`, `AtlasEngine::install`, `fixtures::build_graph`, `examples/demo.rs`); the CLI passes `FlowIndex::default()`. Add new passes inside `derive::all`. Derived edges are pure functions of graph + index: stateless, idempotent, lifecycle via the differ, and never carried forward — `Edge::is_projected()` is the exhaustive match (`TrafficFlow`, `Covers`, `Serves` are not projected), so a new `Edge` must declare its side.
  - `derive::containment`: links each bare `GenericIpAddress` into every CIDR pivot covering it (`Edge::Covers`), so a flow confirms the rule that allowed it.
  - `derive::service`: collapses a path into `Edge::Serves` between typed resources, in traffic direction (`ALB -Serves-> Instance`). One kind, two provenances: a control-plane chain → `inferred`; observed traffic → `confirmed`. A lapsed flow drops a wired target back to `inferred` rather than deleting it. Chains are `Node::kind()` sequences in `CHAINS` (only the AWS LB → TargetGroup → Instance row exists), checked against `ALL_KINDS`; the matcher walks projected edges only. **Direction comes from ports**: request and reply are separate records, so `flow::orientation` picks the service end (one ephemeral port → the other end; neither → the lower port; ambiguous → packet direction) and `FlowStats` keeps a per-pair majority. `TrafficFlow` edges keep packet direction. Confirmation is read from the index, not the edge (an evicted flow's edge can outlive its record).
  - **Address ownership**: a pivot is owned by what `ConnectsTo` it, resolved transitively up `HasIp` (`Instance -HasIp-> Eni -HasIp-> Eip`) with a visited set. An interface nothing holds is its own owner. `ResolvesTo` never confers ownership; an unowned endpoint yields no edge, which keeps internet and NAT peers out.
  - **Status rides the observation channel** (rule 6). Snapshots use `derive::observations` instead of `FlowIndex::observations`; patches use `derive::DerivedObservations::changed`, owned by the reconcile loop, which compares values and sends only what moved. A `Serves` observation has `last_seen` (max over confirming flows; `0` when `inferred`) and status, never `packets`/`bytes`.

`docs/audit_findings.md` records resolved findings as patterns to avoid reintroducing.
