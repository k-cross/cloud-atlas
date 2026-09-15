# Change Monitoring & Live Backend Design

**How Cloud Atlas detects change in each cloud, and how it becomes a push-based live backend.**

> **Reading note.** §§1–8 are the original evaluation, written when Cloud Atlas
> was a batch tool, and are kept in their original tense as the *rationale* for
> the design. For what is actually built, read §9 (the phase table and the two
> "as built" notes under it) and §10 (what is still open). Phases 1–4 are Done;
> Phase 5 — Tiers 1 and 2 for GCP, Azure and Cloudflare — is not.

## 1. Executive Summary

Cloud Atlas today is a batch tool. `AtlasEngine::run_daemon` polls on a fixed
interval and, on every tick, throws away the entire graph (`self.builder =
GraphBuilder::new()`) and rebuilds it from a full re-scan of every provider. That
is fine for a point-in-time `.dot` export, but it contradicts the stated goal of a
"continuous live in-memory digital twin synchronized via event streams." A wipe
-and-rebuild loop cannot tell you *what changed* or *when*, cannot drive
incremental UI updates, and gets more expensive and slower the larger the estate.

This document does two things:

1. **Evaluates the mechanisms** available to detect change in each cloud —
   full-scan polling, control-plane event/audit streams, and data-plane network
   flow logs — and answers the two questions raised: *can flow records alone
   infer liveness and new nodes?* and *can we detect change without a team
   instrumenting their own infrastructure?*
2. **Proposes the backend architecture** that consumes those signals: a
   long-lived server that owns the in-memory graph, applies incremental diffs,
   and pushes patches to a mostly-passive frontend.

**Bottom line up front:** No single signal is sufficient. Flow logs are a
data-plane enrichment, not a topology source of truth. The recommended design is
a **three-tier hybrid**: cloud-native control-plane event streams as the primary
change feed, network flow logs as liveness/edge enrichment, and periodic
full-scan polling as a reconciliation backstop. Critically, all three rely only
on *cloud-managed, read-only* facilities — none require the owning team to add
agents or code to their workloads.

## 2. The Core Question: What Counts as "a Change"?

The digital twin has two layers, and they change through different channels:

| Layer | Examples | Changes via |
|---|---|---|
| **Topology (control plane)** | A VPC/subnet/instance/ENI is created, deleted, retagged, reconfigured; a security group rule changes | Cloud provider API state — the resource *exists* or its config *differs* |
| **Liveness & traffic (data plane)** | An instance is actually sending/receiving packets; a flow to an unknown external IP appears | Observed network activity — the resource is *doing* something |

Confusing these is the central design trap. A newly created instance that never
sends a packet is a **topology** change with zero **data-plane** signal. A busy
instance that gets deleted stops appearing in flow logs only *eventually* and
only *by absence* — a signal you can't distinguish from "idle" for a long time.
This is why the question "can flow records infer new nodes and liveness?" has to
be answered carefully (§4).

## 3. Mechanism A — Full-Scan Polling (the current baseline)

**What it is:** what we do now — call every list/describe API, project the whole
graph, repeat. Enhanced version: keep the previous graph and *diff* instead of
wiping it, emitting add/remove/modify events.

**Verdict: necessary but not sufficient.** Keep it as the reconciliation
backstop, not the primary live feed.

- **Pros:** already implemented; complete and authoritative (it *is* the resource
  state); provider-uniform; self-healing — a full scan repairs any missed event.
  No extra cloud setup or permissions beyond what collectors already use.
- **Cons:** latency is bounded by the poll interval (60s today, and it will grow
  as scans slow on large estates); cost and API-throttling scale with estate size
  × frequency, so you cannot poll your way to low latency; every tick re-reads
  everything to find the handful of things that changed.
- **Required upgrade:** replace the `GraphBuilder::new()` wipe with a persistent
  graph plus a **differ** that compares the freshly projected graph against the
  live one and produces a change set (see §7 change model). This is a prerequisite
  for *every* other tier — even event streams need a reconciliation pass to
  correct drift. *(Built — `atlas::patch::diff`. The daemon and the server both
  install scans through it; neither wipes. What the doc did not anticipate is
  that the differ also needs to be told when a scan is **incomplete**, or a
  failed read is indistinguishable from a deletion — hence `CollectionReport`,
  `carry_forward` and `Retention`.)*

## 4. Mechanism B — Network Flow Records (the user's hypothesis)

> *"Can all changes to liveness or new nodes be inferred by network flow records?"*

**Short answer: no — flow logs are a data-plane enrichment, not a topology
source of truth. They answer "who is talking to whom, and is this thing alive
right now," not "what resources exist and how are they configured."**

The sources are real and require no workload instrumentation:

| Cloud | Flow source | Delivery |
|---|---|---|
| AWS | VPC Flow Logs (ENI-level) | → CloudWatch Logs or S3 → subscription filter / EventBridge → Kinesis |
| GCP | VPC Flow Logs | → Cloud Logging → Log Router sink → Pub/Sub |
| Azure | VNet flow logs (NSG flow logs are being retired) | → Storage → Event Grid, or via Traffic Analytics |
| Cloudflare | Logpush (HTTP/firewall events, not L3 flows) | → object store / HTTP endpoint |

### Why flow logs alone are insufficient

- **New nodes only appear once they emit or receive traffic.** A provisioned-but
  -silent resource is invisible. Worse, a flow log gives you an *IP and an ENI
  ID*, not the rich resource metadata (instance type, tags, subnet association,
  security groups) the graph's typed `Node` variants require. You'd be
  reconstructing topology from shadows.
- **Deletion is only inferable by absence**, which is indistinguishable from
  "idle" without a long, arbitrary timeout — unacceptable for a twin meant to be
  accurate.
- **Flow logs are lossy and delayed:** sampled/aggregated, with multi-minute
  delivery lag (notably AWS's ~10-minute aggregation windows). Fine for
  "liveness within the last few minutes," useless for prompt topology accuracy.
- **They see only L3/L4 traffic**, so config-only changes (a security group rule
  edit, a tag change, an IAM change) produce *no* flow signal at all.

### What flow logs *are* genuinely good for

This is exactly the enrichment §3 of `network_inference_design.md` already
anticipates — it maps cleanly onto the graph:

- **Liveness / health overlay:** "this ENI sent traffic in the last N minutes" →
  a freshness property on the node, feeding the health status the rendering
  design (Phase 3) wants.
- **Edge discovery & weights:** observed flows become `Edge::TrafficFlow`
  (proposed in the inference doc) with `packet_count` / `last_seen`, and flows to
  unknown external IPs become `Node::GenericIpAddress` — the cross-cloud pivot
  that already exists.
- **Confirming inferred edges:** a security-group rule says traffic *could*
  flow; a flow log proves it *does*.

**Conclusion:** flow logs enrich the graph the control plane builds. They cannot
build it.

## 5. Mechanism C — Control-Plane Event & Audit Streams (the recommended primary)

Every major cloud emits a near-real-time, read-only stream of resource-change
events *managed by the cloud itself*. This is the direct answer to the second
question — **yes, we can detect change without the owning team instrumenting
anything.** These are audit/asset facilities the platform already produces; we
only need read access and, in some cases, a one-time sink/subscription setup. No
agents, no sidecars, no code in the customer's workloads.

| Cloud | Primary change feed | What it delivers | Setup burden |
|---|---|---|---|
| **AWS** | **EventBridge** (from CloudTrail management events) and/or **AWS Config** configuration-item stream | Per-resource create/modify/delete events, often with the changed config | Rule → SQS/Kinesis target. Config adds cost but gives full config diffs |
| **GCP** | **Cloud Asset Inventory real-time feeds** (`AssetFeed` → Pub/Sub); Admin Activity **Audit Logs** via Log Router → Pub/Sub | Asset-level change notifications with prior/new state | Create feed + Pub/Sub topic; org- or project-scoped |
| **Azure** | **Event Grid** system topics on subscriptions; **Activity Log**; **Resource Graph** change-analysis | Resource write/delete events; change history queries | Event Grid subscription per scope |
| **Cloudflare** | **Audit Logs API** (poll) / limited webhooks | Account-level config changes | Weakest streaming story — falls back to short-interval polling |

**Verdict: this is the primary live feed for topology and the trigger for
liveness re-evaluation.**

- **Pros:** low latency (seconds to ~1–2 min); event carries *what* changed;
  cheap relative to full scans since you process deltas, not the whole estate; no
  workload instrumentation — purely cloud-managed audit/asset infrastructure.
- **Cons:** per-cloud setup and IAM (a real, but one-time, onboarding cost);
  delivery is at-least-once and occasionally lossy, so it **must** be paired with
  the §3 reconciliation scan; event schemas differ wildly per cloud and must be
  normalized (§7); Cloudflare has no good push story.
- **Instrumentation note:** "requires the team to enable a feed/sink" is *config*,
  not *instrumentation*. We are consuming the platform's own audit trail, not
  asking anyone to modify their applications. This satisfies the constraint.

## 6. Recommendation — Three-Tier Hybrid

No single mechanism wins; they cover different failure modes. Layer them:

```
Tier 1  Control-plane event streams   → primary topology + change triggers  (§5)
Tier 2  Network flow logs             → liveness + traffic edges + weights   (§4)
Tier 3  Full-scan polling             → periodic reconciliation / drift repair (§3)
```

- **Tier 1** drives the graph's structure in near-real-time.
- **Tier 2** decorates it with liveness and observed edges; never used to
  create/delete typed resource nodes.
- **Tier 3** runs on a slow cadence (e.g. every 10–30 min) purely to catch
  dropped events and correct drift — the same code we have now, minus the wipe.

Degradation is graceful: if Tier 1 isn't wired up for a given cloud (or is
Cloudflare), that cloud simply falls back to a faster Tier 3 poll. The system is
always *correct* on the reconciliation interval and *fast* wherever streams
exist.

## 7. Backend Architecture — From Batch Tool to Live Server

The mechanisms above are only useful if something long-lived holds the graph and
pushes deltas. This is the server the user asked about.

```
        ┌────────── per-cloud ingestion adapters ──────────┐
 AWS  ─▶ │ EventBridge/Config  ─┐                            │
 GCP  ─▶ │ Asset feed / audit  ─┤─▶ normalize to ChangeEvent │
 Azure─▶ │ Event Grid          ─┤                            │
 CF   ─▶ │ audit poll          ─┘                            │
 Flow ─▶ │ flow-log consumers  ───▶ LivenessEvent            │
        └───────────────────┬───────────────────────────────┘
                            ▼
                  ┌──────────────────┐   reconciliation
                  │  Graph Actor     │◀── (Tier 3 full scan)
                  │  owns live graph │
                  │  applies diffs   │──▶ GraphPatch (added/removed/updated)
                  └────────┬─────────┘
                            ▼
                  WebSocket / SSE hub  ──▶  frontend (mostly listens)
```

### Key components

- **Ingestion adapters (one per cloud, per tier).** Each translates a
  provider-specific event into a normalized internal `ChangeEvent { resource_id,
  kind, op: Created|Modified|Deleted, payload }`. This is the natural home for the
  existing projector logic, invoked per-resource instead of per-full-scan.
- **The Graph Actor.** A single owner of the live `petgraph` graph (an actor /
  `tokio::task` behind an mpsc channel) so all mutation is serialized and
  lock-free for readers. It converts `ChangeEvent`s into graph mutations via the
  existing `GraphBuilder` dedup logic, and emits a `GraphPatch` describing exactly
  which `NodeIndex`/`EdgeIndex` were added, removed, or had properties changed.
- **The differ (Tier 3).** On each reconciliation scan, project into a *scratch*
  graph and diff against the live one, emitting the same `ChangeEvent` stream the
  adapters produce — so reconciliation and streaming share one mutation path.
- **Push hub.** WebSocket (or SSE — simpler, and traffic is server→client
  dominant) broadcasting `GraphPatch` messages. On connect, the client gets one
  full snapshot (today's `atlas.json`, versioned by `SNAPSHOT_VERSION`) then
  incremental patches. This keeps the frontend "mostly listening," as desired,
  and keeps `atlas-render` decoupled — the contract stays the versioned snapshot
  plus a patch delta of the same shape.

### Required graph-model changes

- **Persistent, incrementally-mutated graph.** Remove the per-tick
  `GraphBuilder::new()` wipe; support `remove_node` / `update_node` alongside the
  current add-only, dedup-on-insert model.
- **Stable identity.** Patches require a stable external key per resource (cloud
  ARN / resource ID / self-link) mapped to `NodeIndex`, so an event can target the
  right node without a full rebuild. The `GraphBuilder`'s `HashMap<Node,
  NodeIndex>` is a starting point but keys on structural equality; we need an
  explicit id→index index that survives property updates.
- **Liveness as node/edge properties**, updated by Tier 2 without touching
  topology — matches the `TrafficFlow` edge and health-overlay direction already
  in the rendering and inference docs. *(Built, but **not** as properties on the
  node or edge — see the Tier-2 note under §9. `Node`/`Edge` are the graph's
  identity types, so liveness lives beside the graph in `FlowIndex`, keyed by the
  same stable wire key.)*

## 8. Evaluation Matrix

| Criterion | A: Full-scan poll | B: Flow logs | C: Event streams |
|---|---|---|---|
| Latency | Poll interval (≥60s, grows) | Minutes (aggregation lag) | Seconds–~2 min |
| Topology completeness | Full & authoritative | Partial, metadata-poor | Full (per-event) |
| Detects config-only change | Yes | No | Yes |
| Detects liveness | No | **Yes (its strength)** | No |
| Cost vs. estate size | Scales badly | Moderate (log volume) | Scales with churn, not size |
| Setup / IAM burden | Already done | Sink + consumer per cloud | Feed/sink + IAM per cloud |
| Workload instrumentation | None | None | None |
| Failure mode | Slow, expensive | Lossy/sampled | At-least-once, occasional drops |
| Role in design | Tier 3 backstop | Tier 2 enrichment | **Tier 1 primary** |

## 9. Phased Implementation Plan

| Phase | Focus | Deliverable | Status |
|---|---|---|---|
| **1** | Incremental graph | Persistent graph + differ; daemon emits change sets instead of wiping. Unlocks everything else with zero new cloud deps. | **Done** — `atlas-lib/src/atlas/patch.rs` (`GraphPatch` + `diff`, keyed on stable `node_key`/`edge_key`); `AtlasEngine::collect` decouples collection from the wipe-and-export CLI path. |
| **2** | Live backend skeleton | Graph Actor + WS hub; frontend consumes snapshot-then-patches. Still driven by Tier 3 polling under the hood. | **Done** — `atlas-server/` (single-writer graph behind `RwLock` + `broadcast`, `poll.rs` Tier-3 reconciliation, bidirectional WebSocket `snapshot`/`patch`/`get_neighbors`); `atlas-web` applies patches live. `--demo` exercises the whole path credential-free. |
| **3** | One real event stream | Wire AWS EventBridge/Config as the first Tier 1 adapter end-to-end; prove the normalized `ChangeEvent` path. | **Done** — `atlas-lib/src/atlas/event.rs` (`ChangeEvent` + `EventApplier`, last-writer-wins on the cloud's own record time); `cloud/amazon/events.rs` (Config configuration items, EC2 state changes, CloudTrail management events → `ChangeEvent`, over an SQS-backed `EventQueue`); `atlas-server/src/stream.rs` + the two-tier `select!` in `poll.rs`. `--aws-event-queue <url>` turns it on. |
| **4** | Flow-log liveness | Tier 2 consumer for VPC Flow Logs → `Edge::TrafficFlow` + node freshness → drives the Phase 3 health overlay in the rendering design. | **Done** — `atlas-lib/src/atlas/flow.rs` (`FlowObservation` + the bounded, expiring `FlowIndex` overlay); `cloud/amazon/flow_logs.rs` (VPC Flow Log records off an S3-notification SQS queue); the Tier-2 arm of `poll::run`, `AppState::flows`, and snapshot v3's `observations`. `--aws-flow-log-queue <url>` turns it on. |
| **5** | Remaining clouds | GCP asset feeds, Azure Event Grid; Cloudflare stays on fast poll. Reconciliation tuned per cloud. | Planned |

> **Tier 1 as built (Phase 3).** EventBridge is the one delivery path and an
> SQS queue is the one consumer, because the server is long-lived and
> restartable: a queue buffers across a restart, and its at-least-once
> redelivery is what `EventApplier` is designed to tolerate. Three producers are
> read off it — AWS Config configuration items (rich enough to project a create
> with its VPC/subnet/security groups attached), EC2 instance state changes
> (free and near-instant, identity only), and CloudTrail management events (the
> broadest coverage, a curated set of mutating calls). Four decisions are
> load-bearing:
>
> - **An adapter may only produce what the full-scan projector produces.** Tier
>   3 diffs the whole graph, so an edge invented by an adapter is deleted at the
>   next reconciliation and re-added by the next event, forever. EC2 instances
>   therefore go through the projector's own `project_instance` (shared with the
>   SDK path via `InstanceFacts`), the Config catch-all node reuses the scan's
>   own `use_aws_resource` filter, and resource types Config reports in a shape
>   the scan would not produce — Route 53 hosted zones (`/hostedzone/` prefix),
>   SQS queues (keyed by URL), and ENIs (keyed correctly by `eni-` id, but the
>   scan only learns about instance-attached ones) — are deliberately left
>   unmapped. A test asserts the event path's subgraph is contained in the full
>   scan's.
> - **Events add; only Tier 3 garbage-collects.** An event speaks for one
>   resource, so a create/modify never removes an edge it failed to mention and a
>   delete removes only the node it named. That asymmetry is what makes a lossy,
>   unordered feed safe to apply at all.
> - **Ordering is last-writer-wins on the cloud's record time**, per resource, so
>   a redelivered create cannot resurrect what a later delete removed. Delivery
>   time is deliberately not used as a fallback: it would assert an ordering the
>   feed does not promise.
> - **Stream health is reported separately from scan health**
>   (`/collection.json`'s `stream` key). A dead feed makes the graph *slow*, not
>   *wrong* — Tier 3 still reads the provider end to end — so it must never enter
>   `unreadable_sources()` and suspend removals. Likewise a message that cannot
>   be parsed is `Malformed`, never a read failure.
>
> The two tiers race, and the resolution is documented rather than hidden: a
> reconciliation installs the estate as of when the scan *started*, so a
> resource an event created mid-scan is removed by that tick and re-added by the
> next. Clients are never inconsistent, only briefly behind; closing it needs
> per-node provenance the graph does not carry.
>
> **Implemented shape vs. this doc:** the live server uses a single-writer
> `RwLock`-guarded graph + `tokio::sync::broadcast` (not a literal actor/mpsc)
> and **WebSocket** (not SSE) — chosen so the client can pull specific data
> (`get_neighbors`) on demand, not just listen. Node *property updates* surface
> as remove + add of the same key for now; richer update semantics land with the
> liveness work. The Graph Actor / event-stream adapters of §7 are the Phase 3+
> build-out on top of this skeleton.

> **Tier 2 as built (Phase 4).** VPC Flow Logs deliver natively to S3 and S3
> notifies SQS natively, so the whole delivery path is operator configuration
> with no code in between — the CloudWatch Logs → subscription filter → Kinesis
> alternative needs a consumer per shard and a Lambda in the middle for no extra
> signal. Four decisions carry the design:
>
> - **The metrics live beside the graph, not inside it.** `Node` and `Edge` are
>   the graph's identity types — hashed, deduplicated on insert, diffed by value
>   — so a packet counter inside `Edge::TrafficFlow` would make every metric
>   update a *different* edge: duplicates past `add_edge`'s dedup, and a
>   remove-then-add of the same `edge_key` in every reconciliation patch. The
>   variant is therefore payload-free and `flow::FlowIndex` holds the numbers,
>   keyed by the same stable `node_key`/`edge_key` the wire uses. That is also
>   the only way node freshness can work, since `Node` cannot carry mutable
>   state either — one mechanism covers both. What the two carry differs:
>   `FlowStats` on an edge has volume, `Liveness` on a node does not. One record
>   names both endpoints plus the instance and interface it came from, so
>   counting its packets against each would report the same traffic four times,
>   and a node's "packets" is an undirected sum across every flow that touched
>   it in any case. `last_seen` is stamped on all of them because it composes as
>   a maximum rather than a sum.
> - **An observation may create only the nodes no provider owns.** A flow record
>   carries an IP, an interface id and a byte count — never a type, tags, subnet
>   or security groups — so a typed node built from one would be topology
>   reconstructed from shadows that the next full scan would disagree with. The
>   exception is the `GenericIpAddress`/`GenericHostname`/`ExternalService`
>   pivots (`Node::owner()` is `None`), which the projectors already emit for
>   addresses they do not recognise either. That exception is what makes a flow
>   between two clouds land as a real edge between their estates.
> - **Expiry deletes, and Tier 3 applies it.** Each reconciliation folds the
>   current overlay into the freshly scanned graph *before* diffing, so a flow
>   that has gone quiet is simply absent from the next graph and the ordinary
>   differ removes its edge. Tier 2 removes nothing itself, exactly as Tier 1
>   garbage-collects nothing. The corollary is that `Edge::TrafficFlow` is the
>   one thing `patch::carry_forward` refuses to hold: a provider going dark says
>   nothing about whether traffic is still flowing, and holding those edges
>   would freeze liveness for as long as any collector is unhealthy.
> - **A third health report, for a third question.** `/collection.json`'s
>   `flows` key sits beside `stream` for the same reason `stream` sits beside
>   the scan: an unreachable flow-log bucket leaves the topology entirely
>   correct and only the liveness stale, so it must never enter
>   `unreadable_sources()`. Its `observed` count is what lets a client tell "we
>   stopped looking" from "the network went quiet".
>
> The overlay is bounded as well as expiring (`DEFAULT_CAPACITY`), because flow
> logs are the highest-volume feed of the three and a busy VPC talks to a
> practically unlimited number of external addresses — each of which would
> otherwise pull a node into the graph for the lifetime of the process.
>
> The snapshot contract went to **v3** for this: `observations` on the snapshot,
> `observations`/`expired` on the patch. Freshness changes far more often than
> topology, so it had to be patchable without re-announcing a resource.

## 10. Open Questions

- **Reconciliation cadence vs. cost** — how slow can Tier 3 run before drift
  becomes noticeable? Likely per-cloud and per-resource-class.
- **Liveness TTL vs. flow-log lag** — `--flow-ttl-secs` defaults to fifteen
  minutes, generous against AWS's one-to-ten-minute aggregation windows plus
  delivery lag. Too short marks healthy resources dark for pipeline reasons;
  too long claims a decommissioned host is still talking. The right value is
  probably per-provider, since GCP and Azure aggregate differently.
- ~~**Generic-node identity is byte-exact, with no normalization.**~~ —
  settled. Every cross-cloud merge in this design, and every flow-log endpoint,
  resolves by a `HashMap<Node, NodeIndex>` lookup on the typed value, so
  `2001:0db8:0000:...:0010` and `2001:db8::10`, an IPv4-mapped form, an
  uppercase or trailing-dot hostname were all *different nodes* and the seam
  silently failed to stitch. `Node::ip` / `Node::hostname`
  (`definition.rs`) are now the only way a pivot is built: they canonicalise
  through `util::canonical_address` (parse to `IpAddr` and re-`Display`,
  unmapping `::ffff:` forms, preserving and re-basing a CIDR prefix) and
  `util::canonical_hostname` (ASCII-lowercase, strip trailing dots). A value
  that will not parse — an AWS prefix-list id, a service tag — is left exactly
  as it came, never guessed at. The fixtures now spell three seams
  *differently* on each side (the v6 flow-log endpoint, the v6 egress CIDR, the
  Route 53 FQDN's trailing dot) so the guard tests fail if a producer goes
  around the constructors.

- ~~**Observed traffic does not confirm inferred reachability.**~~ — settled in
  `atlas::containment`. §4 lists "confirming inferred edges" as one of the things
  flow logs are genuinely good for: a security-group rule says traffic *could*
  flow, a flow record proves it *did*. Security groups project their rules as
  `GenericIpAddress("203.0.113.0/24")` and flow logs produce
  `GenericIpAddress("203.0.113.77")`, so exact-value matching kept them apart.
  `containment::link` now walks the graph's generic pivots once per pass, splits
  them into ranges and addresses, and links each address into every range that
  contains it, so the whole chain reads
  `SecurityGroup -RoutesTo-> 203.0.113.0/24 -Covers-> 203.0.113.77 <-TrafficFlow- 10.10.1.11`.
  The two decisions this document asked for:

  - **Its own edge kind.** `Edge::Covers` asserts something about a *rule*, not
    about traffic, and reusing `RoutesTo` would have made a derived link
    indistinguishable from one a scan actually reported.
  - **Owned by no tier — because it is derived, not observed.** A containment
    edge is a pure function of which pivots are in the graph at that moment, so
    it has no independent lifetime and needs neither `carry_forward`'s ownership
    model nor the flow overlay's expiry. It is *recomputed* at every point a
    graph is finalized, after `carry_forward` and after `FlowIndex::overlay`
    (`poll::reconcile`, `AtlasEngine::install`, `fixtures::build_graph`,
    `examples/demo.rs`). Every lifecycle then falls out of the ordinary differ:
    delete the rule and the range node goes, so the edge goes; let the flow
    lapse and the address goes, so the edge goes.

  The corollary is that `carry_forward` must refuse to hold it, exactly as it
  already refused `TrafficFlow` — otherwise a `Covers` edge would anchor a
  flow-derived address through a provider outage and keep alive the very node
  the overlay had dropped. `Edge::is_projected()` is that distinction, an
  exhaustive match so a new edge kind has to declare which side it is on.

  Range-to-range containment is deliberately **not** built: one rule subsuming
  another is a statement about policy overlap, not about traffic, and it would
  add an edge per nested pair in estates that write a lot of rules.

- **Cross-account/org onboarding** — event feeds need setup per account/project/
  subscription; how do we make enabling them turnkey for an operator?
- ~~**Ordering & idempotency**~~ — settled for Tier 1 in `atlas::event`:
  idempotency falls out of `GraphBuilder`'s dedup, and ordering is
  last-writer-wins per resource keyed on the cloud's own record time, with a
  bounded ordering table so a long-running daemon does not accumulate one entry
  per resource ever seen. Still open for *cross-tier* ordering — a scan cannot
  currently tell that part of its result is older than an event already applied.
- **Backpressure** — a large change burst (mass deploy, region event) must not
  stall the push hub; patches may need coalescing per client.
- **State on restart** — the server is a live in-memory twin; on restart it
  rehydrates via one full Tier 3 scan before opening the stream. Do we also need
  durable persistence, or is re-scan-on-boot enough?
