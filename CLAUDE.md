# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Cloud Atlas builds a **continuous live property graph** of multi-cloud infrastructure. The overarching goal is a live in-memory digital twin synchronized via event streams — not a static point-in-time snapshot. Keep long-running daemon execution in mind when writing code.

## Architecture Rules

1. **Always use strongly typed enums.** The graph is `petgraph::Graph<Node, Edge>`. All node and edge types are defined in `atlas-lib/src/atlas/definition.rs`. Never use raw strings or hashmaps to represent resources.
2. **ENI is the core networking pivot.** Semantic paths start from the Elastic Network Interface: `Instance -> HasIp -> ENI -> AttachedTo -> Subnet`. `Node::AwsEc2Eni` is keyed by the interface's own `eni-` id — the identity every source that mentions an interface actually carries (`DescribeInstances`' `networkInterfaces`, Config items, flow logs' `interface-id`). The owning instance is the `HasIp` edge, never part of the key: an ENI can be reattached elsewhere, an instance can be multi-homed across subnets, and most ENIs (NAT gateways, load balancer nodes, RDS, in-VPC Lambda) belong to no instance at all. An instance that reports no interfaces therefore gets no ENI and no path to its subnet — substituting a direct `Instance -> Subnet` edge would be a shape no other producer emits.
3. **`Display` is required on every new type.** Every new `Node` or `Edge` variant must implement `std::fmt::Display` for clean `.dot` output. Follow the existing `Type::SubType(id)` format pattern.
4. **Never let a failure look like an absence.** This is a live graph, so "we could not read it" and "it is gone" must stay distinguishable all the way to the differ — see the `CollectionReport` contract under Live Server.
5. **Cross-cloud stitching via generic nodes.** Use `Node::GenericIpAddress` and `Node::GenericHostname` as cross-cloud integration points. Connect to them with `Edge::RoutesTo` (traffic) or `Edge::ResolvesTo` (DNS). Graph deduplication is automatic — `GraphBuilder` merges identical generic nodes from different clouds via its `HashMap<Node, NodeIndex>`.
6. **`Node` and `Edge` carry identity, never mutable state.** Both are `Hash + Eq` and that *is* their identity: `GraphBuilder` dedups on it, `patch::diff` compares on it, and `node_key`/`edge_key` derive the wire id from it. A field that changes while the resource stays the same — a packet counter, a `last_seen` — would make every update a different value: duplicates past `add_edge`'s dedup, and a remove-then-add of the same key out of every diff. Such properties go beside the graph, keyed by the stable key. `atlas::flow::FlowIndex` is the one instance, and the reason `Edge::TrafficFlow` is payload-free.

## Testing Without Cloud Credentials

No live cloud credentials are available locally. All projection testing runs against the fake "Globex" environment in `atlas-lib/src/fixtures.rs`, which populates **every collection variant of every provider** plus deliberate cross-cloud seams. Do not write tests that require real cloud API calls.

- `cargo nextest run` — includes exhaustiveness guards: every `Node`/`Edge` kind must appear in the fixture graph. A `Node` variant cannot exist without a line in the `nodes!` list in `definition.rs` — that list *is* the enum — and the guard test then fails until fixtures + a projector actually produce it.
- `cargo run --example demo` — credential-free verification simulation: projects the fixtures, folds the Tier-2 overlay on top, writes `multi_cloud_demo.dot` and a render snapshot carrying its `observations`, prints a per-kind coverage table plus every observed flow, and exits non-zero if any kind is missing *or* the overlay is empty. `fixtures::build_graph()` is the two halves together; `fixtures::topology()` and `fixtures::observed()` are each half on its own, for a consumer that needs the numbers as well as the graph.

When adding a resource type: add one `nodes!` line, the projector mapping, and fixture data — the guard tests enforce all three. One line is the whole `Node` change: `nodes!` generates the enum declaration, `Display`, `kind()`, `ALL_KINDS` and `owner()` from a single grouped list, in the form `Variant(id) => "Provider::SubType({id})"` (struct variants take `Variant { a, b }` and name both fields in the label). The list is grouped by owning `CollectionSource`, so a new variant must be filed under the provider whose scan is authoritative for it — that grouping is what scopes carry-forward on an incomplete scan.

### Collector tests (the HTTP → struct boundary)

Fixtures test **projectors** (they hand-build `Provider` collections), not the **collectors** that fetch and deserialize cloud API responses. Collectors are tested by replaying canned responses — no credentials, no network:

- **reqwest clients (GCP/Cloudflare/Azure):** `wiremock` mock server + a `base_url` seam. Each client has a `with_base_url(token, url)` DI constructor that points every collector at the mock — `GoogleApiClient` (`api/google/client.rs`), `CloudflareApiClient` (`cloud/cloudflare/mod.rs`), `AzureApiClient` (`api/azure/client.rs`). Each example pairs a Layer-1 contract test (deserialize a realistic body) with a Layer-2 test exercising the real pagination + error path (GCP `nextPageToken`, Azure `$skipToken`, Cloudflare's `{success,result}` envelope).
- **AWS SDK (`aws-sdk-*`):** `aws_smithy_runtime`'s `StaticReplayClient` injected into a test `SdkConfig` via `.http_client(...)` — replays a canned response through the *real* SDK deserializer (AWS types aren't `serde`, so this is the only way to test them). See `cloud/amazon/instance.rs` tests.

Per-collector fan-out: GCP/Cloudflare/Azure wiremock tests live in `atlas-lib/tests/{gcp,cloudflare,azure}_collectors.rs`; every AWS collector is covered in `cloud/amazon/collector_tests.rs` (a shared `replay_config` helper feeds canned responses, one per request, in order — must be a unit module since `aws-config` is a normal dep unavailable to integration tests). Azure's `provider::map_resources` is split out from the fetch so the ARG-row → typed-model mapping is testable without `az login`; it returns `(collections, CollectionReport)` and never fails — a row that will not deserialize is skipped and reported (capped at `MAX_REPORTED_ROWS` plus a tail count), because ARG returns the whole tenant in one response and failing the batch on one drifted row would empty the Azure half of the graph.

The `cloudflare`-crate collectors (zone/dns/kv) go through the `cloudflare` crate's own `Client`, not `CloudflareApiClient`, so their seam is `Environment::Custom(format!("{}/client/v4/", server.uri()))` — the crate joins each endpoint's relative `path()` onto that base, so wiremock works there too (same file, `serve_crate`/`crate_client` helpers). Unlike our own all-`Option` models, the crate's result structs are strict (`Zone`, `DnsRecord` have mostly required fields, `DnsContent` is an internally-tagged enum), so a drifted body fails deserialization outright.

Key rule: our own models are all `Option<T>` and serde ignores unknown fields, so a mismatched struct parses into all-`None` and passes a weak "did it parse?" check. **Assert the specific fields the projector reads are populated**, not just that deserialization succeeded.

## Build & Run

```bash
cargo build --release          # binary: target/release/atlas
cargo run -- --regions us-east-1 us-west-2
cargo run -- --gcp-projects my-project
cargo run -- --cloudflare      # requires CLOUDFLARE_API_TOKEN env var
cargo run -- --azure-subscriptions sub-id
cargo run -- --daemon          # polls every 60 seconds
cargo run -- --verbose         # verbose output
```

Output is written to `atlas.dot` plus a render snapshot `atlas.json` in the working directory (both gitignored). Visualize `.dot` with Gephi.

## Dev Orchestration (`cargo xtask`)

The unified way to run and test the whole stack (server + wasm renderer + frontend) — prefer these over hand-running the pieces:

```bash
cargo xtask dev --demo         # full stack, credential-free: wasm (rebuilt if stale) →
                               #   atlas-server :4681 → frontend :4680, one Ctrl-C stops all
cargo xtask dev                # same, real collection (default; add provider flags as needed)
cargo xtask wasm [--force]     # rebuild atlas-web/static/pkg/ if atlas-layout sources are
                               #   newer (do this after any SNAPSHOT_VERSION bump)
cargo xtask demo               # regenerate multi_cloud_demo.json from fixtures
cargo xtask test [--e2e]       # every suite in order: nextest (root) → nextest (atlas-render) →
                               #   biome lint → svelte-check → bun unit [→ playwright]
```

**cargo-nextest is the default Rust test runner.** `cargo xtask test` runs
`cargo nextest run --all-targets` in both workspaces, falling back to
`cargo test --all-targets` when cargo-nextest isn't installed (`cargo install
cargo-nextest --locked`). The printed `▶` line shows which runner was used.
Two things to know:

- `--all-targets` is load-bearing: nextest skips `examples/` by default, and it
  is what keeps `atlas-lib/examples/demo.rs` and `atlas-layout`'s
  `layout_demo.rs` compile-checked the way plain `cargo test` did.
- **nextest never runs doctests.** The repo has none today; if you add one, it
  needs a separate `cargo test --doc`.

Shared config lives in `.config/nextest.toml` — one per workspace (root and
`atlas-render/`), since nextest resolves config from the workspace root. The
`default` profile is fail-fast with a 30s slow-test warning; a `ci` profile
(`cargo nextest run -P ci`) runs the full suite with one retry.

`xtask` (root workspace, alias in `.cargo/config.toml`) only shells out to the same commands listed below — it adds ordering, a readiness gate (frontend waits for `/snapshot.json`), staleness checking for the wasm engine, and teardown of the whole process tree.

## Live Server (`atlas-server/`)

`atlas-cli` is the batch/one-shot path. `atlas-server` is the long-running **live backend** (Phase 2 of `docs/change_monitoring_design.md`): it owns a persistent in-memory graph, reconciles it against the providers on an interval (Tier-3 polling, reusing `AtlasEngine::collect`), diffs each scan (`atlas::patch::diff`), and pushes incremental `GraphPatch`es to the frontend over WebSocket. It never wipes the graph — the differ is the incremental path the daemon lacks.

**Collection outcomes are part of the contract.** `AtlasEngine::collect` returns a `Scan { builder, report }`, where the `CollectionReport` lists every source that could not be read (`atlas::collection`). A failed fetch must never reach the projector as an empty collection: the differ would read the absence as deletion and broadcast a removal for every node that source owns, which the next healthy tick puts straight back. When a report names sources it could not read, `poll::reconcile` folds the live graph forward for **those sources only** (`patch::carry_forward` + `report.unreadable_sources()`), so the tick is additive-only inside the failed provider's territory while every healthy provider stays authoritative, deletions included. That scoping is what makes the graph converge: a collector that fails on every tick must not stop a genuinely deleted resource in *another* cloud from ever being removed. Ownership is `Node::owner()`, generated from the grouped variant list in `definition.rs`; nodes with no owner (`GenericIpAddress`/`GenericHostname`/`ExternalService` — the cross-cloud stitching points) are retained on any incomplete scan, since any provider may still reference them. The CLI daemon applies the identical policy in `AtlasEngine::install` before writing `atlas.dot`/`atlas.json`.

**Retention is a budget, not a promise** (`patch::Retention`). Holding resources is the right answer to a transient failure and the wrong answer to a permanent one: a collector that fails on every tick would pin its resources forever, and "unconfirmed since start-up" is not a live twin. A source is held for `--retain-scans` consecutive incomplete scans (default 10, so ten minutes at the default poll interval) and released on the next one, letting the differ delete what the scans could not confirm; recovery resets the streak. Both the server loop and the CLI daemon run the same `Retention`; only the server exposes the budget as a flag, the CLI daemon uses `Retention::default()`.

**The budget depends on why the read failed** (`collection::FailureKind`), because a stringified error cannot tell a recoverable failure from a permanent one. Every failure is classified once, at the call site where the error is still typed — never inferred later from a message:

- `Unavailable` — throttling, timeouts, 5xx. Held for the full `--retain-scans` budget. This is what `run_all` records, because a collector's error arrives boxed and its kind is no longer recoverable there; it is the conservative reading.
- `Unauthorized` — missing/expired/insufficient credentials. Held for the much shorter `Retention::AUTH_BUDGET` (2), since polling will not fix it without a human, and continuing to claim unverifiable resources for ten minutes is worse than dropping them. Never longer than `--retain-scans`, so `--retain-scans 0` still retains nothing. A provider diagnoses this *before* fanning out, where the error is still typed: GCP's `authenticate()`, Cloudflare's `clients()`, Azure's `AzureApiClient::new()`, and — since the AWS SDK resolves credentials lazily at the first request — `amazon::resolve_credentials`, probed once per region so a bad chain is one accurate `Unauthorized` instead of sixteen boxed `Unavailable`s.
- `Malformed` — a drifted row in a response that *was* read. **Not an unreadable source**: it never enters `unreadable_sources()`, never triggers carry-forward, and holds nothing. This is why the distinction exists — ARG answers for the whole Azure tenant in one response, so classifying an unmappable row as a read failure froze deletions across every Azure resource for the full budget because one resource drifted.

A source counts as `Unauthorized` only when *every* failure that made it unreadable was a refusal (`report.unreadable_kind`); mixed evidence keeps the longer budget, since releasing early is only safe when the diagnosis is unambiguous. Note `is_complete()` is stricter than "authoritative": a scan carrying only `Malformed` failures is incomplete but still trusted for removals. Note the bluntness this trades for: a long total outage releases that provider's whole unconfirmed estate in one patch. Finer, collector-level retention is *not* derivable from the node type — an `AwsEc2Vpc` is produced by five different AWS collectors — so it would need per-node provenance recorded during projection. Providers signal this by returning `ProviderScan { provider, report }`. **`build_*` is infallible** — there is no second failure channel: a provider that dies at the credential step still returns a scan, with the empty collection explained by its report (`"credentials"`, `"auth"`), and a partial read returns what it got plus a failure per unreachable scope (a throttled AWS collector, one unreachable Cloudflare zone). Never swallow a collector error with `.ok()` or `if let Ok(..)` — record it on the report.

```bash
cargo run -p atlas-server -- --demo                  # credential-free: serves Globex fixtures
                                                     #   with a churning sentinel, port 4681
cargo run -p atlas-server -- --regions us-east-1     # real collection (same flags as the CLI)
cargo run -p atlas-server -- --poll-secs 30 --port 8080
cargo run -p atlas-server -- --retain-scans 3          # give up on an unreadable
                                                       #   provider after 3 scans
cargo run -p atlas-server -- --aws-event-queue https://sqs.us-east-1.amazonaws.com/111/atlas-events
                                                       # Tier-1 live change feed (below)
cargo run -p atlas-server -- --aws-flow-log-queue https://sqs.us-east-1.amazonaws.com/111/atlas-flows
                                                       # Tier-2 liveness overlay (below)
cargo run -p atlas-server -- --flow-ttl-secs 300       # how long an observed flow
                                                       #   counts as current; unset, the
                                                       #   collection source answers
```

### Demo mode is a module, not a mode flag threaded through the code

`--demo` is one `Source` variant and one module. **Everything the credential-free
run actually does lives in `atlas-server/src/demo.rs`** — the sentinel pair that
flips by tick parity, the burst-flow cadence, the seed graph, and how long a
demo observation counts as current. `poll::Source` only chooses between `Live`
and `Demo` and delegates; `main.rs` holds no demo constants at all.

The rule this protects: **a demo detail must never become a production default.**
`--flow-ttl-secs` is the worked example. The demo needs a short TTL so its burst
flow visibly lapses, but writing that as "under `--demo`, default to three poll
intervals" would have put a demo cadence inside the flag that configures a real
deployment, and into its `--help`. Instead the flag is simply unset-able, and
`Source::default_flow_ttl` asks the source: a real feed answers
`FlowIndex::DEFAULT_TTL`, the demo answers `demo::flow_ttl(poll)`. Adding a
third source would answer for itself too, and neither the flag nor the loop
would change.

The one thing deliberately *not* in this module is the Globex data itself
(`fixtures::flows`, `fixtures::burst_flow`). Fixture data belongs with the rest
of the fake environment in `atlas-lib`, where the projector tests use it too;
what belongs here is the *behaviour* — when it is observed, and for how long it
counts.

`demo::graph` returns `fixtures::topology()`, **not** `fixtures::build_graph()`,
and the difference is the whole tier split. `build_graph` folds the overlay into
the graph it returns, which is right for a static snapshot and wrong for a scan:
traffic baked into the scanned topology is traffic the differ can never remove,
so the demo would draw flow edges that no `FlowIndex` backs and that no TTL can
expire. Instead the demo's flows reach the graph the way a real deployment's do
— through the index, folded in by `poll::reconcile` — and `Source::seed_flows`
primes a fresh index at start-up so the first snapshot still carries them
without waiting a poll interval. `the_scan_graph_carries_no_traffic_of_its_own`
pins it.

### Tier 1: the live event feed (`--aws-event-queue`)

Polling is the backstop, not the primary feed. `atlas::event` is the normalized
change model — a `ChangeEvent` naming one resource, what happened to it, and the
neighbourhood the payload described — and `EventApplier` folds those into the
live graph between reconciliation scans. `cloud/amazon/events.rs` is the first
real adapter: an EventBridge rule targets an SQS queue, and `EventQueue` reads
AWS Config configuration items, EC2 instance state changes and CloudTrail
management events off it. `poll::run` is a `select!` over the reconciliation
ticker and both live feeds, so all three tiers share one writer and mutation
stays serialized. Four rules hold this together — break any of them and the two
tiers start undoing each other:

1. **An adapter may only produce nodes and edges the full-scan projector would
   also produce.** Tier 3 diffs the whole graph and is authoritative, so an edge
   invented by an adapter is deleted at the next reconciliation and re-added by
   the next event, forever. EC2 instances therefore go through the projector's
   own `projector::aws::project_instance` (shared with the SDK path via
   `InstanceFacts` — three wire shapes, one definition of how an instance
   attaches), and the Config catch-all node reuses the scan's own
   `use_aws_resource`/`use_global` filters. A resource type whose identity the
   graph keys differently from Config is deliberately **not** mapped rather than
   mapped approximately: ENIs (keyed correctly, but the full scan only learns
   about interfaces attached to a described *instance*, so a Config item for a
   NAT gateway's ENI would be a node no scan produces — this arm opens up once a
   `DescribeNetworkInterfaces` collector exists), Route 53 hosted zones
   (`/hostedzone/` prefix), SQS queues (keyed by URL). The
   `an_instance_event_produces_only_what_a_full_scan_would` test pins this by
   projecting the same instance both ways and asserting containment.
2. **Events add; only Tier 3 garbage-collects.** A create/modify never removes
   an edge it did not mention, and a delete removes only the node it named. That
   asymmetry is what makes an at-least-once, out-of-order feed safe to apply.
3. **Ordering is last-writer-wins on the cloud's own record time**, tracked per
   resource, so a redelivered create cannot resurrect what a later delete
   removed. Arrival time is never substituted for a timestamp we cannot parse —
   that would assert an ordering the feed does not promise. The ordering table
   is bounded (this is a daemon, not a script).
4. **Stream health is not scan health.** A dead feed makes the graph *slow*, not
   *wrong* — Tier 3 still reads the provider end to end — so stream failures
   live in `AppState::stream_report` and surface under `/collection.json`'s
   `stream` key. They must never enter `unreadable_sources()`, or an unreachable
   queue would suspend deletions across all of AWS. Likewise a message that will
   not parse is `FailureKind::Malformed` (reported, then deleted so a poison
   message cannot replay forever), never a read failure.

Known trade, tested so it stays known: the tiers race. A reconciliation installs
the estate as of when the scan *started*, so a resource an event created
mid-scan is removed by that tick and re-added by the next. Clients are never
inconsistent, only briefly behind.

Adapter tests replay canned EventBridge bodies (`cloud/amazon/events/tests.rs`),
and the SQS transport is covered end to end with `StaticReplayClient` — queue
message → `ChangeEvent` → graph mutation → `GraphPatch`, no credentials.

### Tier 2: the liveness overlay (`--aws-flow-log-queue`)

The control plane says what exists; only flow logs say whether any of it is
doing anything. `atlas::flow` is that overlay — `FlowObservation` is the
normalized record every provider's flow feed translates into, and `FlowIndex`
is the bounded, expiring store the live server keeps beside the graph
(`AppState::flows`). `cloud/amazon/flow_logs.rs` is the first adapter: VPC Flow
Logs deliver to S3, S3 notifies an SQS queue, and the queue is drained,
gunzipped and parsed. Four rules hold it together:

1. **Metrics live beside the graph, not inside it** — architecture rule 6
   above. `Edge::TrafficFlow` means only "traffic was seen here"; `FlowIndex`
   holds the numbers, keyed by `node_key`/`edge_key`. Node freshness works the
   same way because it has no other option. Volume and freshness are split
   deliberately: a flow edge gets `FlowStats` (`last_seen` + `packets`/`bytes`),
   a node gets `Liveness` (`last_seen` + verdict) and **no volume**. One record
   names up to four keys, so counting its packets against each would report the
   same traffic four times, and a node's "packets" would be an undirected sum
   across every flow touching it regardless. `last_seen` survives that treatment
   because it composes as a maximum, not a sum.
2. **An observation may create only the nodes no provider owns.** A flow record
   carries an IP and an interface id, not a type, tags, subnet or security
   groups, so a typed node built from one is topology reconstructed from
   shadows. `GenericIpAddress` and its siblings (`Node::owner()` is `None`) are
   the exception — the projectors already emit them for addresses they did not
   recognise either, which is what makes a cross-cloud flow land as a real edge
   between two estates. A typed endpoint the scan has not found is *skipped*,
   never invented.
3. **Expiry deletes, and Tier 3 applies it.** `poll::reconcile` folds
   `FlowIndex::overlay` into the scanned graph before diffing, so a flow that
   goes quiet is simply absent from the next graph and the ordinary differ
   removes its edge. The corollary: `Edge::TrafficFlow` is the one thing
   `patch::carry_forward` refuses to hold, because a provider going dark says
   nothing about whether traffic is still flowing.
4. **Flow health is its own report** (`AppState::flow_report`, `/collection.json`'s
   `flows` key). An unreachable bucket makes liveness stale and leaves the
   topology entirely correct, so it must never enter `unreadable_sources()` —
   the same rule as the Tier-1 `stream` report, one tier along.

Record parsing reads the object's own header line for the field layout and only
falls back to the version-2 default order when there is none: flow-log format is
chosen field by field, and assuming the default is how a custom format silently
reads bytes as ports. A record with no `action` column yields
`FlowObservation::action == None` (status `observed`) rather than a fabricated
verdict. `interface-id` and `instance-id` are each read straight off the record
and never derived from one another: the first is in the v2 default set and is the
subject of every record, the second needs a v3 format and is absent for every
interface that belongs to something other than an instance. Neither buys a node —
the overlay attributes freshness only to resources a scan already found.

Adapter tests replay canned flow-log objects and a full SQS + S3 conversation
through `StaticReplayClient` (`cloud/amazon/flow_logs/tests.rs`); the overlay's
own policy is covered in `atlas/flow.rs`. `--demo` re-observes
`fixtures::flows()` every tick, so the whole path runs credential-free, and
adds `fixtures::burst_flow()` on one tick in four — the only part of the demo
that shows the whole lifecycle, since it goes quiet and the *reconciliation*
differ is what removes it. **That cadence lives in `atlas-server/src/demo.rs`,
not in the reconciliation loop** — see Demo mode below.

The frontend renders the overlay rather than merely carrying it: a flow edge
takes its verdict's color and a log-scaled width from its packet count, every
node heard from gets a pulsing halo, and packets animate along each flow edge
on a separate `canvas.traffic-layer` above Sigma's own — bright beads over the
edge's own hue, since a bead in the edge's colour vanishes into a wide one.
Bead count and transit time are both log-scaled off `packets`: real volumes
span orders of magnitude, and a linear mapping either saturates at the cap or
leaves every flow at one bead. That canvas is excluded
from the "settled graph is rock-still" e2e regression on purpose — its motion
is the feature, and a second test asserts it *does* change frame to frame.

- `GET /snapshot.json` — full current snapshot (v3: nodes, edges, and the flow overlay's `observations`). `GET /collection.json` — `complete` (did the scan lose anything at all), `unreadable` (which sources could not be read, and so are suspending removals), `failures` (each attributed to its source/scope and stamped with its `kind`), `stream` (the Tier-1 feed's own health) and `flows` (the Tier-2 feed's, plus how many flows are currently observed) — the last two deliberately separate, see above. This is the only way a client can tell "this provider holds nothing" from "this provider could not be reached" from "we read it but dropped a row" — it matters most at start-up, when an outage makes the first partial collection the baseline. `GET /ws` — WebSocket hub.
- WS is **bidirectional**: server pushes `snapshot` then `patch`es; the client can pull `get_snapshot` / `get_neighbors` on demand.
- Point the frontend at it: run `bun dev` in `atlas-render/atlas-web/` (assets on :4680) which connects by default to `ws://<host>:4681/ws`; override with `?server=ws://…` or force offline with `?static`.

## Rendering Workspace (`atlas-render/`)

Interactive rendering (`docs/graph_rendering_design.md`) lives in a **separate cargo workspace** — `atlas-render/` is `exclude`d from the root workspace and must never depend on `atlas-lib` (the cloud SDK tree doesn't build for wasm, and rendering stays decoupled from graph building). The only contract is the versioned render snapshot JSON (and the `GraphPatch` delta of the same shape). It now has **three consumers** that pin `SNAPSHOT_VERSION` (currently **v3**, which added the Tier-2 `observations` list on the snapshot and `observations`/`expired` on the patch): the producer `atlas-lib/src/atlas/export.rs`, the Rust layout consumer `atlas-render/atlas-layout/src/graph.rs`, and the TS frontend `atlas-render/atlas-web/src/graph.ts`. When the shape changes, **bump the version in all three and rebuild the wasm** (`bun run wasm` in `atlas-render/atlas-web/`) — the compiled layout engine bakes in the version and rejects mismatched snapshots at runtime.

- `atlas-layout` — pure-Rust ForceAtlas2 (Barnes-Hut, deterministic, flat `f32` position buffer); `parallel` feature enables rayon natively.
- `atlas-layout-wasm` — wasm-bindgen bridge; builds with `cargo build -p atlas-layout-wasm --target wasm32-unknown-unknown`.
- `atlas-web` — Sigma.js WebGL frontend, a **bun** app (use bun, not node/npm): `bun install && bun run wasm && bun dev` inside `atlas-render/atlas-web/` serves at `http://localhost:4680`. By default it connects to `atlas-server` over WebSocket (`ws://<host>:4681/ws`) for a live snapshot-then-patches feed; with no server it falls back to a static `/snapshot.json` fetch (or force that with `?static`).
- Test with `cargo nextest run` **inside `atlas-render/`** (the root run does not cover it — it is a separate workspace). Static end-to-end without credentials: `cargo run --example demo` (root) → `cargo run --example layout_demo -- ../multi_cloud_demo.json` (in `atlas-render/`) → `bun dev` (view at `http://localhost:4680/?static`). Live end-to-end: `cargo run -p atlas-server -- --demo` (root) + `bun dev` (in `atlas-render/atlas-web/`) to watch patches apply as the demo graph churns, including one burst flow per four ticks that arrives, pulls in the endpoint node no scan owns, then lapses and is removed by the differ.

## Auth (reference only — not available locally)

| Provider | Mechanism |
|---|---|
| AWS | Standard credential chain (`~/.aws/credentials`, env vars, instance role) |
| GCP | OAuth2 browser flow via `yup-oauth2` — opens browser on first run |
| Cloudflare | `CLOUDFLARE_API_TOKEN` env var — required, hard-errors if missing |
| Azure | `az login` (`AzureCliCredential`) |

## Rust Edition

Rust **2024 edition** (`let`-chain syntax). Requires rustup stable ≥ 1.85.

## VCS

Uses **jj (Jujutsu)** on top of git. Typical workflow: `jj describe` → `jj new` → `jj squash` → `jj git push --change <id>` for stacked PRs on GitHub.

## Established Helpers (use these, don't re-duplicate)

- Per-scope fan-out: `cloud::collector::run_all` — hand it the region's/project's `Vec<NamedCollector<'_, T>>` and it runs them concurrently, keeping each collector's name attached to its outcome so a failure lands in the report as `{scope}/{name}`. Both AWS and GCP build that list with a local `collectors!` macro: one line per collector, no parallel join/destructure to keep in sync. In both, the line carries the *variant* as well as the call, and the macro applies it — a collector returns its own natural type (`Vec<Instance>`, `AWSNetworking`) and never names the enum itself. That pairing is what makes the registration type-checked: a collector registered against a variant its return type does not fit fails to compile, so no collector can file its results under another one's name. Multi-field payloads therefore need a named struct (`AWSLoadBalancing`, `AWSRoute53`, `AWSNetworking` in `cloud/definition.rs`) rather than an inline struct variant, since only a *path* can be applied as a function.
- AWS: `cloud/amazon.rs::load_config(region)` — SDK config is loaded once per region in `provider.rs` and passed as `&SdkConfig` to collectors. Register a new collector as one `"name" => AmazonCollection::Variant, runner(..)` line in the `collectors!` list in `amazon/provider.rs`.
- GCP: `GoogleApiClient::paginated_list` in `api/google/client.rs` — every GCP list endpoint goes through it (handles auth, paging, errors). Register a new one as `"name" => GoogleCollection::Variant, call(..)` in `google/provider.rs`'s `collectors!` list.
- Azure: `AzureApiClient::query_graph` treats a response with no `data` **array** as an error, never as an empty tenant — table-format results, a missing or null `data`, and an error body delivered with a 2xx all used to return `Ok` with zero rows, which reads as a complete scan of an empty tenant and deletes every Azure node in the graph. It also errors when `totalRecords` claims more records than came back. Keep that asymmetry if you touch it: too few rows is data loss, too many is harmless.
- Azure: the `azure_types!` list in `azure/provider.rs` is the single source for both the ARG `where type in~ (..)` filter and `map_resources`' dispatch. Add a resource type there and the exhaustive match makes the compiler demand its mapping arm; `leaf!` covers the `{id, name, location}` case in one line.
- Cloudflare: `CloudflareApiClient::get` in `cloud/cloudflare/mod.rs` — for raw REST endpoints not covered by the `cloudflare` crate (`get_paged` also hands back `result_info`, which cursor-paginated endpoints like R2 need). `cloudflare::paginate` walks any page-numbered crate endpoint to exhaustion: pass `per_page` and a closure taking the page number. Never terminate a page loop on "the page came back short" — a clamped `per_page` makes that an ordinary response, and stopping there silently truncates the collection into what the differ reads as mass deletion.
- Projectors: `projector::project_parallel` is the single definition of the per-scope fan-out — hand it the slice and a closure and it projects each item into its own `GraphBuilder` across rayon, then merges them back in order. AWS, GCP and Azure all go through it; Cloudflare has one collection and does not. `project_leaf!` macro in `projector/mod.rs` (shared by all four projectors — `use super::project_leaf;`) for resources that only add a node. Two forms: `(builder, items, field, Node::Variant)` reads a struct field and adds a standalone node (GCP/Azure serde models), `(builder, items, accessor(), Node::Variant, parent, edge)` calls an accessor and links to a parent (AWS SDK models).
- Projector edge idiom: `GraphBuilder::link_to(from, node, edge)` / `link_from(node, to, edge)` are the one-call form of "get-or-add this node and connect it", and are what projectors should use — they replaced the ~177 hand-written `get_or_add_node` + `add_edge` pairs. Reach for `get_or_add_node`/`get_or_add_ref` directly only when you need the `NodeIndex` for more than one edge.
- `GraphBuilder::add_edge` deduplicates identical edges automatically, and `GraphBuilder::merge(&graph)` is the single definition of folding one graph into another by node identity — `patch::carry_forward` is a thin policy wrapper over it, so never hand-roll a node/edge dedup pass. `GraphBuilder::remove_node` is the matching removal: it reports every edge that died with the node (the patch needs to name them) and repairs `node_map` after petgraph's swap-remove, which silently moves the last node onto the removed index. Never call `graph.remove_node` directly on a builder-owned graph.
- Event adapters: `atlas::event::ChangeEvent` is the normalized shape every provider's live feed translates into, and `EventApplier` is the only thing that applies one (idempotency + ordering live there, not at the call sites). A new adapter builds its `context` subgraph with the *projector's* own functions — see `projector::aws::project_instance` and `InstanceFacts` — so it cannot emit a shape the full scan would disagree with.
- Flow adapters: `atlas::flow::FlowObservation` is the Tier-2 equivalent, and `FlowIndex` is the only thing that stores one — the admission rule (which endpoints may become nodes), expiry and the capacity bound all live there, not at the call sites.
- Cross-cloud pivots: `Node::ip(value)` / `Node::hostname(value)` are the only
  way to build a `GenericIpAddress`/`GenericHostname` — never name the variant
  at a producer. They canonicalise through `atlas::util::canonical_address` and
  `canonical_hostname`, because these nodes resolve by exact value through
  `GraphBuilder`'s `HashMap<Node, NodeIndex>`: without one spelling,
  `2001:0db8:0000:…:0010` from a flow-log writer and `2001:db8::10` from a DNS
  API are two pivots and the seam silently fails to stitch, with no error
  anywhere. A value that will not parse (an AWS prefix-list id, a service tag)
  is passed through untouched rather than guessed at.
- `patch::merge_additions` is the single definition of "fold this subgraph into the live graph and report only what was genuinely new". Both live tiers use it; novelty has to be measured before the merge, since `GraphBuilder::merge` dedups silently.

`docs/audit_findings.md` records resolved audit findings — patterns to avoid reintroducing. The live tiers' entries are the sharpest of them: a flow-log notification is deleted only once its objects were actually *read* (Tier 1 can delete unconditionally, Tier 2 cannot — it has a network fetch in that gap), the between-scans merge context is bounded by what `FlowIndex` retained rather than by the batch that arrived, eviction cuts on a tie without collapsing the index, and an interface whose subnet the event did not report attaches to nothing rather than to a guess.
