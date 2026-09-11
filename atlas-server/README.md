# atlas-server

The Cloud Atlas **live backend**: a long-running server that owns a persistent
in-memory graph, reconciles it against the cloud providers on an interval, and
pushes incremental patches to the frontend over WebSocket. This is Phase 2 of
[`docs/change_monitoring_design.md`](../docs/change_monitoring_design.md); the
CLI (`atlas-cli`) remains the batch/one-shot path.

## Run

```bash
cargo xtask dev --demo                              # the whole stack (server + frontend), supervised
cargo run -p atlas-server -- --demo                 # just this server: credential-free fixtures, port 4681
cargo run -p atlas-server -- --regions us-east-1    # real collection (same provider flags as the CLI)
cargo run -p atlas-server -- --poll-secs 30 --port 8080
cargo run -p atlas-server -- --retain-scans 3       # give up on an unreadable provider after 3 scans
cargo run -p atlas-server -- \
  --aws-event-queue    https://sqs.us-east-1.amazonaws.com/111/atlas-events \
  --aws-flow-log-queue https://sqs.us-east-1.amazonaws.com/111/atlas-flows \
  --flow-ttl-secs 300
```

| Flag | Default | What it does |
|---|---|---|
| `--regions` / `--gcp-projects` / `--azure-subscriptions` / `--cloudflare` | `us-east-1` | Provider scope, identical to the CLI |
| `--port` | `4681` | HTTP + WebSocket listener |
| `--poll-secs` | `60` | Tier-3 reconciliation interval |
| `--retain-scans` | `10` | Consecutive incomplete scans a source's resources survive before the differ is allowed to delete them |
| `--aws-event-queue` | off | SQS URL for the Tier-1 control-plane feed |
| `--aws-flow-log-queue` | off | SQS URL for the Tier-2 flow-log feed (S3 notifications) |
| `--flow-ttl-secs` | source-chosen | How long an observed flow counts as current. Unset, a live feed uses `FlowIndex::DEFAULT_TTL` (15 min) and `--demo` uses a short demo cadence |
| `--demo` | off | Credential-free fixtures instead of real collection |
| `--verbose` | off | Verbose collection logging |

`--demo` serves the fake multi-cloud fixtures, flips a sentinel node/edge in and
out every other tick, re-observes the fixture flows every tick, and adds one
burst flow on every fourth tick — so the whole snapshot → patch → apply path,
including a flow that arrives and later lapses, is exercised with no cloud
credentials. Everything demo-specific lives in `demo.rs`, never as a default on a
production flag.

## Design

- **Single-writer graph.** The poll loop (`poll.rs`) is the only mutator, behind
  an `Arc<RwLock<Graph>>`; WebSocket connections are readers. Patches fan out via
  a `tokio::sync::broadcast` channel. `poll::run` is a `select!` over the
  reconciliation ticker and both live feeds, so all three tiers share that one
  writer and mutation stays serialized.
- **Tier 1 — control-plane events** (`stream.rs`, `atlas_lib::atlas::event`).
  EventBridge/Config/CloudTrail messages off an SQS queue become normalized
  `ChangeEvent`s and are folded in between scans. Events only ever *add*; a
  delete removes just the node it named. Ordering is last-writer-wins on the
  cloud's own record time, so a redelivered create cannot resurrect what a later
  delete removed. An adapter may only produce nodes and edges the full-scan
  projector would also produce — otherwise the next reconciliation deletes them
  and the next event re-adds them, forever.
- **Tier 2 — flow-log liveness** (`atlas_lib::atlas::flow`, `AppState::flows`).
  Observations land in a bounded, expiring `FlowIndex` beside the graph, never
  inside `Node`/`Edge`. `reconcile` folds the current overlay into the scanned
  graph *before* diffing, so a flow that goes quiet is simply absent next tick
  and the ordinary differ removes its edge. Tier 2 deletes nothing itself.
- **Tier-3 reconciliation.** Each tick re-derives the graph
  (`AtlasEngine::collect`, or fixtures in demo), diffs it against the live graph
  (`atlas_lib::atlas::patch::diff`, keyed on stable `node_key`/`edge_key`), and
  broadcasts only non-empty `GraphPatch`es.
- **A failure never looks like an absence.** `AtlasEngine::collect` returns the
  scan *and* a `CollectionReport`. When a source could not be read, `reconcile`
  carries the live graph forward **for that source's nodes only**
  (`patch::carry_forward` + `Node::owner()`), so the tick is additive-only inside
  the failed provider's territory while every healthy provider stays
  authoritative. Retention is a budget, not a promise: a source is held for
  `--retain-scans` incomplete scans (or the shorter `Retention::AUTH_BUDGET` when
  every failure was a credential refusal) and then released, because
  "unconfirmed since start-up" is not a live twin.
- **Three separate health questions.** Scan health, stream health and flow health
  are reported independently. A dead event feed or an unreachable flow-log bucket
  makes the graph *slow* or *stale*, not *wrong* — Tier 3 still reads the provider
  end to end — so neither may enter `unreadable_sources()` and suspend removals.

## API

- `GET /snapshot.json` — the full current snapshot (`SNAPSHOT_VERSION` 3: nodes,
  edges, and the Tier-2 `observations` overlay).
- `GET /collection.json` — `complete`, `unreadable` (sources currently suspending
  removals), `failures` (each attributed to its source/scope and stamped with its
  `FailureKind`), `stream` (Tier-1 feed health) and `flows` (Tier-2 feed health
  plus how many flows are currently observed). This is the only way a client can
  tell "this provider holds nothing" from "this provider could not be reached"
  from "we read it but dropped a row".
- `GET /ws` — WebSocket hub. Bidirectional JSON frames:
  - client → server: `{"type":"subscribe"}`, `{"type":"get_snapshot"}`,
    `{"type":"get_neighbors","key":"…"}`
  - server → client: `{"type":"snapshot",…}`, `{"type":"patch",…}`,
    `{"type":"neighbors",…}`, `{"type":"error","message":…}`

A patch carries `observations` (refreshed liveness) and `expired` (lapsed keys)
alongside added/removed nodes and edges, so freshness updates without
re-announcing a single resource.

CORS is permissive so the `atlas-render/atlas-web` dev server (`bun dev` on
:4680) can connect to `ws://localhost:4681/ws`.

## Tests

`cargo nextest run -p atlas-server` — 44 tests covering the WebSocket protocol,
the reconciliation/retention policy, both live-tier arms of the poll loop, and
the stream back-off. No credentials or network required.
