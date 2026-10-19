# Change Monitoring & Live Backend Design

Cloud Atlas synchronizes its in-memory graph through a **three-tier hybrid** built entirely on read-only, cloud-managed feeds—no workload agents required.

## 1. Three-Tier Architecture

The digital twin separates **topology** (resource existence and configuration, driven by control-plane APIs) from **liveness** (observed packet flow, driven by data-plane logs). Neither signal suffices alone: a new idle instance emits no flow logs, while a deleted busy instance disappears from flow logs only by silence.

| Property | Tier 1: Event Streams | Tier 2: Flow Logs | Tier 3: Full-Scan Polling |
|---|---|---|---|
| **Role** | Primary topology changes | Liveness & observed traffic | Authoritative reconciliation & GC |
| **Latency** | Seconds to ~2 min | Minutes (batched delivery) | Poll interval (default 60 s) |
| **Topology** | Incremental per event | None (IP + interface ID only) | Complete & self-healing |
| **Config-only changes** | Yes | No | Yes |
| **Liveness** | No | **Yes** | No |
| **Cost driver** | Resource churn | Traffic log volume | Estate size × poll frequency |
| **Failure mode** | At-least-once, unordered, lossy | Delayed, sampled | Throttling, partial outages |
| **Rights** | Add/update + explicit node delete | Add ownerless pivots & flow stats | Full diff + garbage collection |

A provider without Tier 1 or Tier 2 runs on Tier 3 alone—always converging on the poll interval.

## 2. Cloud Feeds

### Tier 1: Control-Plane Events

| Cloud | Source & Delivery | Status |
|---|---|---|
| **AWS** | EventBridge (CloudTrail management events, AWS Config items, EC2 state changes) → SQS (`--aws-event-queue`) | Built (`atlas::event`, `cloud/amazon/events.rs`) |
| **GCP** | Cloud Asset Inventory feeds + Admin Activity audit logs (Log Router) → Pub/Sub | Planned |
| **Azure** | Event Grid system topics, Activity Log, Resource Graph change history | Planned |
| **Cloudflare** | Audit Logs API (no push feed; relies on fast polling) | Polling only |

### Tier 2: Network Flow Logs

Flow logs enrich topology with liveness ("this ENI talked in the last $N$ minutes"), traffic volume, external IP pivots, and security-rule confirmation (`Covers`). They never invent typed resources.

| Cloud | Source & Delivery | Status |
|---|---|---|
| **AWS** | VPC Flow Logs (ENI level) → S3 → SQS (`--aws-flow-log-queue`) | Built (`atlas::flow`, `cloud/amazon/flow_logs.rs`) |
| **GCP** | VPC Flow Logs → Cloud Logging → Log Router → Pub/Sub | Planned |
| **Azure** | VNet flow logs → Storage → Event Grid | Planned |
| **Cloudflare** | Logpush (L7 HTTP/firewall events, not L3) | Not applicable |

## 3. Backend Pipeline (`atlas-server`)

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

- **Single writer:** `poll::run` `select!`s over the reconciliation ticker and both live feeds. Readers share an `Arc<RwLock<Graph>>`; patches fan out via `tokio::sync::broadcast` over WebSocket (chosen over SSE so clients can send `get_neighbors` / `get_snapshot`).
- **Normalised adapters:** Tier-1 adapters emit `ChangeEvent` using the same projector helpers as full scans; `EventApplier` enforces per-resource last-writer-wins ordering by cloud record time. Tier-2 adapters emit `FlowObservation` into the bounded, expiring `FlowIndex`.
- **Failure ≠ absence:** Tier-3 scans return `CollectionReport` alongside the graph. Unreadable sources are retained via `patch::carry_forward` for `--retain-scans` ticks (fewer for `Unauthorized`) rather than diffed as deletions. Stream and flow health report separately on `/collection.json`.
- **Stable wire keys:** Patches address elements by `node_key`/`edge_key` (derived from `Node`/`Edge` identity). Attribute changes surface as remove + add; high-frequency liveness rides the sideband `observations` and `expired` lists (snapshot v3).
- **Stateless boot:** Restarting the server rehydrates from a single full scan without durable storage.

Historical rationale (SQS buffering, S3 → SQS vs. Kinesis, bounded index capacity, scan-start race) is recorded in [`docs/audit_findings.md`](audit_findings.md).

## 4. Next Steps & Open Questions

- **Phase 5 (Multi-cloud feeds):** Implement GCP Cloud Asset Inventory (Pub/Sub) and Azure Event Grid Tier-1 adapters, plus GCP and Azure Tier-2 flow log ingestion into `FlowObservation`.
- **Reconciliation cadence:** Tune Tier-3 polling intervals per cloud and resource class once Tier-1 streams are active.
- **Liveness TTL:** `FlowIndex::DEFAULT_TTL` (15 min) accommodates AWS S3 delivery lag; evaluate per-provider defaults so healthy resources do not go dark and decommissioned hosts expire promptly.
- **Cross-tier ordering:** Track per-node provenance so a Tier-3 scan cannot overwrite a newer Tier-1 event that arrived after scan start.
- **Onboarding:** Package Terraform/CloudFormation templates for turnkey EventBridge/S3/SQS, Pub/Sub, and Event Grid setup.
- **Backpressure:** Coalesce patches per WebSocket client if change bursts stall slow consumers.
- **Persistence:** Evaluate whether cold-start full scans remain fast enough on large estates without a local snapshot cache.
