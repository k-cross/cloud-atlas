# Demo Design

The credential-free demo showcases Cloud Atlas without cloud accounts. **Keep this document current:** any change that adds or modifies a tier, edge kind, derivation, or failure mode must update the coverage table or follow-ups below.

## Surfaces

| Command | Role |
|---|---|
| `cargo run --example demo` | Fixture health check: projects `fixtures::build_graph()`, prints kind coverage, traffic overlay, and derived edges, writes `multi_cloud_demo.{dot,json}`, and exits non-zero if any kind is missing or the overlay is empty. |
| `atlas-server --demo` | Serves the Globex fixtures live (`atlas-server/src/demo.rs`), polling every 5 s (`demo::POLL`) unless `--poll-secs` is set, and plays the scripted loop below. The second tick of each cycle (`demo::BURST_TICK`) injects a rejected SSH burst from `192.0.2.99` to `web-01` that expires three polls later. |
| `cargo xtask dev --demo` | Runs Wasm build (if stale), `atlas-server --demo`, and the web UI. |
| `cargo xtask demo` | Regenerates `multi_cloud_demo.json` and `atlas-web/static/snapshot.json` (used by `/?static` and offline mode; auto-run by `dev --demo` when older than any crate it is built from). |

## Capability Coverage

| Capability | Shown Today | Follow-up |
|---|---|---|
| All `Node` and `Edge` kinds projected | Yes (exhaustiveness guards + CLI example) | |
| Cross-cloud stitching (public IPs & DNS) | Yes, static (Route 53 & Cloudflare → Azure App Service & Cloud Run; AWS flows → Azure LB frontend & GCE NAT IP) | |
| Per-network private address scoping | Yes, static (`vpc-globex-staging` reuses `10.10.0.0/16` with its own isolated `10.10.1.10`); the two VPN flows resolve across tunnels (below) | |
| Cross-network private resolution | Yes, static (`web-01` → warehouse via `pcx-globex-data`; `web-02` → shared-services ENI via `tgw-globex`; `web-02` → west API interface across `tgw-globex` ↔ `tgw-globex-west` peering; Azure & GCP peerings projected) | |
| Address spaces & route tables | Yes, static (every network's ranges; VPC routes and the transit gateways' associations, propagations and static routes gate each crossing; blackhole routes on both route types) | |
| Hubs | Yes, static for transit gateway peering (above); Virtual WAN `vwan-globex` and NCC `globex-hub` are drawn and joined but no flow crosses them | |
| Site-to-site VPN | Yes, static (AWS `vgw-globex` ↔ Azure hub gateway, with the spoke reaching it by gateway transit: `web-01` → Azure VM; `tgw-globex` ↔ GCP HA VPN: `web-02` → Cloud SQL) | |
| Dedicated circuits | Yes, static (Direct Connect ↔ GCP Cross-Cloud Interconnect: warehouse → Cloud SQL; ExpressRoute ↔ Cross-Cloud Interconnect is drawn and joined, but no Azure flow log feeds it) | |
| Tier 3 scan → diff → patch | Yes (web-04 launches and terminates through the AWS projector) | |
| Tier 2 traffic overlay | Yes (19 steady flows with verdicts, volume, and beads; periodic burst flow) | |
| `Covers` derivation | Yes, static (`203.0.113.0/24` covers `203.0.113.77`) | |
| `Serves` (`inferred` vs. `confirmed`) | Yes, live (web-04 `inferred` → `confirmed`; web-01 `confirmed` → `inferred` → `confirmed`; the static snapshot has 13 `confirmed`, 2 `inferred`) | |
| Service view | Yes | |
| Failure semantics (`CollectionReport`, `carry_forward`, `Retention`) | Yes, live (Azure is unreadable for three scans, carried forward with no topology change, and shown in the Collection panel as `holding n/10 scans`) | |
| Search & focus | Yes (search box, `/` shortcut, results, fly-to focus with neighbours lit) | |
| Tier 1 events | No (`--aws-event-queue` is unused in demo mode) | F6 |

**Baseline metrics:** 226 nodes and 323 edges in 17 components (the main one holds 199 nodes; the Virtual WAN, its hub and its two VNets form a 6-node island), 13 edgeless nodes, 8 region-only leaves, labels up to 156 characters.

## Scripted Loop

`demo::SCRIPT` repeats every `demo::CYCLE` (16) ticks; a stage's length is set by the flow TTL (`FLOW_TTL_TICKS`, 3) so each transition is visible:

| Stage | Ticks | Scan | Traffic | What it shows |
|---|---|---|---|---|
| `Steady` | TTL + 1 | Globex | All steady flows | Baseline; on re-entry web-04 is terminated and Azure is back, so their edges go. Outlasts the TTL so web-04's old traffic lapses before it relaunches. |
| `Launched` | 2 | + web-04 (instance, ENI, target registration) | Steady | `ALB -Serves-> web-04` appears `inferred` |
| `Serving` | 2 | + web-04 | + ALB ↔ web-04 | web-04 promotes to `confirmed` |
| `Quiet` | TTL + 2 | + web-04 | − everything touching web-01's addresses | web-01's flows lapse: `ALB -Serves-> web-01` reverts to `inferred`; its flow-only `Serves` edges go |
| `Outage` | 3 | + web-04, no Azure collections, Azure `Unavailable` in the report | as `Quiet` | Azure held by `carry_forward` with an empty topology patch; Collection panel shows the outage |

Tests in `demo.rs` replay the loop through `carry_forward`, `FlowIndex` and `derive::all` on a clock that advances one poll per tick (`play`), asserting each stage's statuses, the empty outage patch, recovery, and that the graph never stands still for more than five ticks. The live Playwright tests (`the scripted outage is held, reported, and recovers`, `a launched instance is wired, then used, and a quiet one falls back`) watch the same loop through a real server.

## Invariants

- **Projector-only shapes:** Demo mutations project fixture data (`fixtures::with_web_04` folds web-04 into the AWS scan; the outage omits Azure's collections and reports it, as `build_azure` does), never hand-built edges (`the_live_change_is_a_shape_a_scan_produces`).
- **Fixture data in `atlas-aws` (read through `atlas_engine::fixtures`), cadence in `demo.rs`:** Poll interval and flow TTL come from `Source::default_poll` / `default_flow_ttl` so demo constants never leak into live defaults. Stage lengths are counted in TTLs of three polls, so `--demo` ignores `--flow-ttl-secs` with a warning; `--poll-secs` scales the whole loop.
- **Traffic via `FlowIndex`:** `demo::graph` returns `fixtures::topology()`, never baking flows into the scan graph (`the_scan_graph_carries_no_traffic_of_its_own`).
- **Continuous churn:** The graph changes at least every five ticks (`the_graph_changes_at_least_once_every_few_ticks`), which the Playwright tests `applies live patches as the demo graph churns` and `patches pin existing nodes` wait on.

## Follow-ups

- **F3. Connect isolated guard nodes:** 13 edgeless nodes (10 Azure, 3 GCP) and 8 region-only AWS leaves exist solely for exhaustiveness guards and push the main component into the centre of the camera. Link GCP DNS zones, functions, and buckets to their project, and evaluate Azure container grouping.
- **F4. Short node labels:** Display concise labels (last ID segment or bare IP) on the canvas and full `Display` keys on hover (Rendering Phase 3).
- **F6. Tier 1 in demo mode (open):** Evaluate simulating between-scan events (e.g. `RunInstances` for web-04's launch) so the demo shows the event feed alongside the scripted scans.
