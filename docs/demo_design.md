# Demo Design

The credential-free demo showcases Cloud Atlas without cloud accounts. **Keep this document current:** any change that adds or modifies a tier, edge kind, derivation, or failure mode must update the coverage table or follow-ups below.

## Surfaces

| Command | Role |
|---|---|
| `cargo run --example demo` | Fixture health check: projects `fixtures::build_graph()`, prints kind coverage, traffic overlay, and derived edges, writes `multi_cloud_demo.{dot,json}`, and exits non-zero if any kind is missing or the overlay is empty. |
| `atlas-server --demo` | Serves the Globex fixtures live (`atlas-server/src/demo.rs`), polling every 5 s (`demo::POLL`) unless `--poll-secs` is set. Odd ticks project `canary.globex.io → 198.51.100.42` via the Cloudflare projector; even ticks drop it. Every fourth tick injects a rejected SSH burst from `192.0.2.99` to `web-01` that expires three polls later. |
| `cargo xtask dev --demo` | Runs Wasm build (if stale), `atlas-server --demo`, and the web UI. |
| `cargo xtask demo` | Regenerates `multi_cloud_demo.json` and `atlas-web/static/snapshot.json` (used by `/?static` and offline mode; auto-run by `dev --demo` when older than `atlas-lib`). |

## Capability Coverage

| Capability | Shown Today | Follow-up |
|---|---|---|
| All `Node` and `Edge` kinds projected | Yes (exhaustiveness guards + CLI example) | |
| Cross-cloud stitching (public IPs & DNS) | Yes, static (Route 53 & Cloudflare → Azure App Service & Cloud Run; AWS flows → Azure LB frontend & GCE NAT IP) | |
| Per-network private address scoping | Yes, static (`vpc-globex-staging` reuses `10.10.0.0/16` with its own isolated `10.10.1.10`); two interconnect flows land on unheld local-scope IPs | F2 |
| Cross-network private resolution | Yes, static (`web-01` → warehouse via `pcx-globex-data`; `web-02` → shared-services ENI via `tgw-globex`; Azure & GCP peerings projected) | |
| Tier 3 scan → diff → patch | Trivially (canary DNS record toggle) | F1 |
| Tier 2 traffic overlay | Yes (17 steady flows with verdicts, volume, and beads; periodic burst flow) | |
| `Covers` derivation | Yes, static (`203.0.113.0/24` covers `203.0.113.77`) | |
| `Serves` (`inferred` vs. `confirmed`) | Static only (9 `confirmed`, 2 `inferred`; no live status transition) | F1 |
| Service view | Yes | |
| Failure semantics (`CollectionReport`, `carry_forward`, `Retention`) | No (demo report is always complete; UI does not surface `/collection.json`) | F1, F5 |
| Tier 1 events | No (`--aws-event-queue` is unused in demo mode) | F6 |

**Baseline metrics:** 144 nodes across 18 components (largest: 112), 13 edgeless nodes, 8 region-only leaves, labels up to 149 characters.

## Invariants

- **Projector-only shapes:** Demo mutations project fixture data (`fixtures::canary`), never hand-built edges (`the_live_change_is_a_shape_a_scan_produces`).
- **Fixture data in `atlas-lib`, cadence in `demo.rs`:** Poll interval and flow TTL come from `Source::default_poll` / `default_flow_ttl` so demo constants never leak into live defaults.
- **Traffic via `FlowIndex`:** `demo::graph` returns `fixtures::topology()`, never baking flows into the scan graph (`the_scan_graph_carries_no_traffic_of_its_own`).
- **Continuous churn:** Every scan mutates the graph (required by the Playwright test `applies live patches as the demo graph churns`).

## Follow-ups

- **F1. Scripted five-step loop (replaces canary toggle):**
  1. `web-04` launches (instance, ENI, IP) and joins the ALB target group → `inferred` `Serves` edge appears.
  2. Traffic reaches `web-04` → promotes to `confirmed`.
  3. `web-01` traffic stops → reverts to `inferred`.
  4. Azure fails (`Unavailable`) → estate carried forward, `/collection.json` reports the outage (visible via F5).
  5. Azure recovers and `web-04` terminates → edges removed.
- **F2. Interconnect private hops:** Resolve the two cross-cloud private flows (`web-01` → Azure NIC, `web-02` → Cloud SQL) once `network_connectivity_design.md` Phase 2 lands.
- **F3. Connect isolated guard nodes:** 13 edgeless nodes (10 Azure, 3 GCP) and 8 region-only AWS leaves exist solely for exhaustiveness guards and push the main component into the centre of the camera. Link GCP DNS zones, functions, and buckets to their project, and evaluate Azure container grouping.
- **F4. Short node labels:** Display concise labels (last ID segment or bare IP) on the canvas and full `Display` keys on hover (Rendering Phase 3).
- **F5. Collection health in the UI:** Render `/collection.json` status in `atlas-web` so degraded/retained sources are visible.
- **F6. Tier 1 in demo mode (open):** Evaluate simulating between-scan events (e.g. `RunInstances`) if F1 leaves a gap.
