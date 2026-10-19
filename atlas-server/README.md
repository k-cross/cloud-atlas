# atlas-server

Live backend owning the persistent in-memory graph, reconciling against providers on an interval, consuming live event and flow-log feeds, and pushing incremental `GraphPatch`es over WebSocket.

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
| `--regions` / `--gcp-projects` / `--azure-subscriptions` / `--cloudflare` | `us-east-1` | Provider scope (same as CLI) |
| `--port` | `4681` | HTTP + WebSocket listener |
| `--poll-secs` | per source | Tier-3 reconciliation interval (live: `60`, demo: `5`) |
| `--retain-scans` | `10` | Consecutive incomplete scans a source's resources survive before release |
| `--aws-event-queue` | off | SQS URL for Tier-1 control-plane events |
| `--aws-flow-log-queue` | off | SQS URL for Tier-2 VPC Flow Log S3 notifications |
| `--flow-ttl-secs` | per source | Flow observation TTL (live: 15 min; demo: scaled to poll) |
| `--demo` | off | Serve Globex fixtures (`demo.rs`: canary DNS toggle + periodic burst flow) |
| `--verbose` | off | Verbose collection logging |

## Architecture

- **Single writer:** `poll::run` is the sole mutator of `Arc<RwLock<Graph>>`, `select!`ing over the reconciliation ticker and both live feeds; patches fan out over `tokio::sync::broadcast`.
- **Tier 1 (`stream.rs`, `atlas::event`):** Normalises SQS messages into additive `ChangeEvent`s (explicit node deletes only), ordered per resource by cloud record time.
- **Tier 2 (`atlas::flow`):** Stores flow records in the bounded, expiring `FlowIndex`. `reconcile` folds `FlowIndex::overlay` into each scan before diffing so expired flows are removed by `patch::diff`.
- **Tier 3 (`poll::reconcile`):** Collects, carries forward unreadable sources for `--retain-scans` ticks (fewer for `Unauthorized`), computes derived edges (`derive::all`), diffs on stable keys, and broadcasts non-empty patches.
- **Isolated health reports:** Scan, stream, and flow health report independently on `/collection.json`; a degraded stream never blocks scan deletions.

## HTTP & WebSocket API

- `GET /snapshot.json` — Snapshot v3 (`nodes`, `edges`, `observations`).
- `GET /collection.json` — `complete`, `unreadable`, `failures` (with `FailureKind`), `stream`, `flows`.
- `GET /ws` — JSON frames tagged by `type`:
  - Client → Server: `subscribe`, `get_snapshot`, `get_neighbors` (`key`)
  - Server → Client: `snapshot`, `patch` (`observations` + `expired` + node/edge diffs), `neighbors`, `error`

## Tests

`cargo nextest run -p atlas-server` — covers WebSocket protocol, retention/carry-forward policy, Tier-1/Tier-2 ingestion, and stream backoff without cloud credentials.
