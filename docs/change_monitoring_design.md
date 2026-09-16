# Change Monitoring & Live Backend Design

How Cloud Atlas detects change in each cloud, and how the live backend consumes it.

## 1. Summary

No single signal is enough. Cloud Atlas uses a **three-tier hybrid**, all built on cloud-managed, read-only facilities — nothing needs agents or code in customer workloads:

```
Tier 1  Control-plane event streams  → primary topology changes          (§4)
Tier 2  Network flow logs            → liveness and observed traffic     (§3)
Tier 3  Full-scan polling            → reconciliation and drift repair   (§2)
```

Enabling a feed or sink is configuration, not instrumentation. A cloud without Tier 1 falls back to Tier 3 alone: always correct on the poll interval, fast wherever streams exist.

The twin has two layers that change through different channels. **Topology** (a resource exists or its config differs) changes via provider APIs. **Liveness** (a resource is sending packets) changes via the data plane. A new silent instance is a topology change with no data-plane signal; a deleted busy instance disappears from flow logs only by absence, which looks like idleness for a long time.

## 2. Full-scan polling (Tier 3)

Call every list/describe API, project the whole graph, and diff it against the live one (`atlas::patch::diff`).

- **Pros:** complete and authoritative; provider-uniform; self-healing (repairs any missed event); needs no extra permissions.
- **Cons:** latency bounded by the interval; cost and throttling scale with estate size × frequency.

The differ must also know when a scan is **incomplete**, or a failed read is indistinguishable from a deletion — hence `CollectionReport`, `carry_forward` and `Retention` (see `CLAUDE.md`).

## 3. Flow logs (Tier 2)

Flow logs cannot build topology — only enrich it.

| Cloud | Source | Delivery |
|---|---|---|
| AWS | VPC Flow Logs (ENI level) | S3 → SQS (built), or CloudWatch Logs → Kinesis |
| GCP | VPC Flow Logs | Cloud Logging → Log Router → Pub/Sub |
| Azure | VNet flow logs (NSG flow logs are retiring) | Storage → Event Grid |
| Cloudflare | Logpush (HTTP/firewall events, not L3) | object store / HTTP |

**Not a topology source:** a resource appears only once it sends traffic; a record carries an IP and interface id, not type, tags, subnet or security groups; deletion is visible only by absence; delivery is aggregated and delayed (AWS up to ~10 min); config-only changes produce nothing.

**Good for:** liveness ("this ENI talked in the last N minutes"), observed edges and their volume, flows to unknown external addresses (generic pivots), and confirming that a rule which *allows* traffic is actually used.

## 4. Control-plane event streams (Tier 1)

| Cloud | Feed | Setup |
|---|---|---|
| AWS | EventBridge (CloudTrail management events), AWS Config items, EC2 state changes | Rule → SQS (built) |
| GCP | Cloud Asset Inventory feeds → Pub/Sub; Admin Activity audit logs via Log Router | Feed + topic, org or project scope |
| Azure | Event Grid system topics; Activity Log; Resource Graph change history | Event Grid subscription per scope |
| Cloudflare | Audit Logs API (poll) | No real push story — use fast polling |

- **Pros:** seconds-to-minutes latency; events say what changed; cost scales with churn, not estate size.
- **Cons:** per-cloud setup and IAM; at-least-once and occasionally lossy (so Tier 3 remains mandatory); schemas differ per cloud and must be normalized.

## 5. Evaluation

| | Full-scan poll | Flow logs | Event streams |
|---|---|---|---|
| Latency | Poll interval | Minutes | Seconds–~2 min |
| Topology | Full, authoritative | Partial, metadata-poor | Full, per event |
| Config-only changes | Yes | No | Yes |
| Liveness | No | **Yes** | No |
| Cost vs. estate | Scales badly | Log volume | Churn |
| Failure mode | Slow, expensive | Lossy, sampled | Duplicates, drops |
| Role | Tier 3 | Tier 2 | **Tier 1** |

## 6. Backend

```
 AWS/GCP/Azure/CF ─▶ per-cloud adapters ─▶ ChangeEvent ─┐
 Flow logs        ─▶ flow adapters      ─▶ FlowIndex  ──┤
 Tier-3 scan      ─▶ collect + project  ─▶ diff ────────┤
                                                        ▼
                               atlas-server: single-writer graph (RwLock)
                                                        │ GraphPatch (broadcast)
                                                        ▼
                                 WebSocket hub ─▶ frontend (snapshot, then patches)
```

- **Adapters** normalize provider events into `ChangeEvent`, reusing projector functions per resource.
- **Single writer.** `poll::run` `select!`s over the reconcile ticker and both feeds; readers share an `RwLock`, patches go out on `tokio::sync::broadcast`. WebSocket rather than SSE so clients can request data (`get_neighbors`).
- **Stable identity.** Patches address elements by `node_key`/`edge_key`, derived from the typed value. Attribute changes surface as remove + add of the same key; mutable data lives in observations instead.
- **Restart** rehydrates with one full scan; there is no durable store.

## 7. Phases

| Phase | Deliverable | Status |
|---|---|---|
| 1 | Persistent graph + differ | Done — `atlas/patch.rs` |
| 2 | Live server, snapshot-then-patches | Done — `atlas-server/` |
| 3 | First Tier-1 adapter (AWS) | Done — `atlas/event.rs`, `cloud/amazon/events.rs`, `--aws-event-queue` |
| 4 | Tier-2 flow logs (AWS) | Done — `atlas/flow.rs`, `cloud/amazon/flow_logs.rs`, `--aws-flow-log-queue`, snapshot v3 |
| 5 | GCP asset feeds, Azure Event Grid, GCP/Azure flow logs; per-cloud reconciliation tuning | Planned |

The load-bearing rules for Tiers 1 and 2 are in `CLAUDE.md`. Design reasons not recorded there:

- **SQS as the Tier-1 consumer.** The server restarts; a queue buffers across restarts, and its at-least-once redelivery is what `EventApplier` is built to tolerate. Config items are rich enough to project a create with its attachments; EC2 state changes are free and immediate but identity-only; CloudTrail has the broadest coverage.
- **S3 → SQS for Tier 2.** Both hops are native AWS configuration with no code in between. The CloudWatch → Kinesis route needs a per-shard consumer and a Lambda for no extra signal.
- **Bounded overlay** (`DEFAULT_CAPACITY`). Flow logs are the highest-volume feed, and a busy VPC talks to effectively unlimited external addresses, each of which would otherwise pin a node for the life of the process.
- **Snapshot v3.** Freshness changes far more often than topology, so `observations`/`expired` had to be patchable without re-announcing resources.
- **Tier race.** A scan installs the estate as of its start, so a resource an event created mid-scan is removed and re-added one tick later. Clients are briefly behind, never inconsistent.

## 8. Open questions

- **Reconciliation cadence** — how slow can Tier 3 run before drift shows? Probably per cloud and resource class.
- **Liveness TTL** — `FlowIndex::DEFAULT_TTL` is 15 minutes, generous against AWS's aggregation plus delivery lag. Too short marks healthy resources dark; too long keeps decommissioned hosts talking. Likely per provider.
- **Cross-tier ordering** — a scan can't tell that part of its result is older than an applied event; needs per-node provenance.
- **Onboarding** — feeds need per-account/project/subscription setup; how can that be turnkey?
- **Backpressure** — a large burst must not stall the push hub; patches may need per-client coalescing.
- **Persistence** — is re-scan-on-boot enough?
