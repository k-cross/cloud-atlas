# atlas-server

The live backend: owns a persistent in-memory graph, reconciles it against the providers on an interval, and pushes incremental patches over WebSocket. `atlas-cli` remains the one-shot path.

## Run

```bash
cargo xtask dev --demo                              # server + frontend, supervised
cargo run -p atlas-server -- --demo                 # credential-free fixtures on :4681
cargo run -p atlas-server -- --regions us-east-1    # real collection
cargo run -p atlas-server -- \
  --aws-event-queue    https://sqs.us-east-1.amazonaws.com/111/atlas-events \
  --aws-flow-log-queue https://sqs.us-east-1.amazonaws.com/111/atlas-flows \
  --flow-ttl-secs 300
```

| Flag | Default | Meaning |
|---|---|---|
| `--regions` / `--gcp-projects` / `--azure-subscriptions` / `--cloudflare` | `us-east-1` | Provider scope, as in the CLI |
| `--port` | `4681` | HTTP + WebSocket listener |
| `--poll-secs` | `60` | Tier-3 reconciliation interval |
| `--retain-scans` | `10` | Incomplete scans a source's resources survive before the differ may delete them |
| `--aws-event-queue` | off | SQS URL for the Tier-1 control-plane feed |
| `--aws-flow-log-queue` | off | SQS URL for Tier-2 flow-log S3 notifications |
| `--flow-ttl-secs` | per source | How long an observed flow counts as current (live: 15 min; demo: short) |
| `--demo` | off | Serve fixtures instead of collecting |
| `--verbose` | off | Verbose collection logging |

`--demo` flips a sentinel node in and out every other tick, re-observes the fixture flows every tick, and adds a burst flow every fourth, so the full snapshot → patch path — including a flow that arrives and lapses — runs without credentials. All of it lives in `demo.rs`.

## Design

- **Single writer.** `poll::run` is the only mutator of an `Arc<RwLock<Graph>>`, `select!`ing over the reconcile ticker and both live feeds; patches fan out on a `broadcast` channel.
- **Tier 1** (`stream.rs`, `atlas_lib::atlas::event`): SQS messages become `ChangeEvent`s applied between scans. Events only add; a delete removes only the named node; ordering is last-writer-wins on the cloud's record time.
- **Tier 2** (`atlas_lib::atlas::flow`): observations go into a bounded, expiring `FlowIndex`. `reconcile` folds the overlay into each scan before diffing, so a quiet flow's edge is removed by the ordinary differ.
- **Tier 3**: each tick collects (or loads fixtures), diffs against the live graph on stable keys, and broadcasts non-empty `GraphPatch`es.
- **Failure ≠ absence.** For each unreadable source, `reconcile` carries that source's nodes forward for `--retain-scans` scans (fewer for credential failures), then releases them.
- **Three health reports.** Scan, stream and flow health are separate; a dead feed makes the graph slow or stale, never wrong, so it never suspends removals.

## API

- `GET /snapshot.json` — snapshot v3: nodes, edges, `observations`.
- `GET /collection.json` — `complete`, `unreadable`, `failures` (with `FailureKind`), `stream`, `flows`.
- `GET /ws` — JSON frames tagged by `type`:
  - client → server: `subscribe`, `get_snapshot`, `get_neighbors` (`key`)
  - server → client: `snapshot`, `patch`, `neighbors`, `error`

Patches carry `observations` (refreshed) and `expired` (lapsed keys) alongside node/edge changes. CORS is permissive so the dev frontend on :4680 can connect.

## Tests

`cargo nextest run -p atlas-server` — WebSocket protocol, reconcile/retention policy, both live-feed arms, stream back-off. No credentials needed.
