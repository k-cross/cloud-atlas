# Demo

The credential-free demo is the only way to see Cloud Atlas without cloud accounts, so it should show what the system does, not just that it runs. This document tracks how far it does. **Keep it current:** a change that adds or alters a capability — a tier behaviour, an edge kind, a derivation, a failure mode — updates the coverage table, either with how the demo shows it or with a follow-up below.

## Surfaces

| Command | What it is |
|---|---|
| `cargo run --example demo` | Fixture health check: projects `fixtures::build_graph()`, prints kind coverage, the traffic overlay and derived edges, writes `multi_cloud_demo.{dot,json}`, exits non-zero if a kind is missing or the overlay is empty. |
| `atlas-server --demo` | The Globex fixtures served live (`atlas-server/src/demo.rs`). Polls every 5 s unless `--poll-secs` says otherwise (`demo::POLL`). Each odd scan adds the Cloudflare record `canary.globex.io → 198.51.100.42` through the real projector and each even scan drops it; a rejected SSH burst from `192.0.2.99` to web-01 arrives one scan in four and lapses three polls later. |
| `cargo xtask dev --demo` | The above plus the web UI. |
| `cargo xtask demo` | Regenerates `multi_cloud_demo.json` and refreshes `atlas-web/static/snapshot.json`, the UI's `?static` and offline view; `dev --demo` does the same when either is older than `atlas-lib`. |

## Coverage

| Capability | Shown today | Follow-up |
|---|---|---|
| Every node and edge kind projected | Yes — exhaustiveness guards and the CLI example | |
| Cross-cloud stitching on public addresses and DNS | Yes, static: Route 53 and Cloudflare onto Azure App Service and Cloud Run; AWS traffic onto the Azure balancer's frontend and the GCE NAT address | |
| Private addresses scoped per network | Yes, static: `vpc-globex-staging` reuses prod's `10.10.0.0/16` with its own `10.10.1.10` node and is joined to nothing (its peering request was rejected). The two interconnect flows still end at addresses nothing holds | F2 |
| Joined networks resolve private addresses | Yes, static: web-01 → the warehouse across `pcx-globex-data`, web-02 → the shared-services endpoint across `tgw-globex`; Azure and GCP peerings are drawn | |
| Tier 3 scan → diff → patch | Trivially: the canary record | F1 |
| Tier 2 traffic overlay | Yes: 17 steady flows with verdicts, volume and beads; the burst comes and goes | |
| `Covers` | Yes, static: `203.0.113.0/24` covers `203.0.113.77` | |
| `Serves`, inferred vs confirmed | Static, 9 confirmed / 2 inferred; no transition is ever shown | F1 |
| Service view | Yes | |
| Failure semantics (`CollectionReport`, carry-forward, retention) | No: the demo's report is always complete, and the UI has no view of `/collection.json` | F1, F5 |
| Tier 1 events | No: `--aws-event-queue` is ignored in demo mode | F6 |

Baseline, 2026-10-18 after network connectivity phase 1, for the follow-ups to move: 144 nodes in 18 components (the largest 112), 13 with no edges, 8 more hanging only off a region; labels up to 149 characters.

## Constraints on any demo change

- **Only shapes a scan produces.** Demo changes go through the projectors with fixture data (`fixtures::canary` is the pattern), never by linking nodes by hand. Pinned by `the_live_change_is_a_shape_a_scan_produces`.
- **Data in `atlas-lib`, behaviour in `demo.rs`** (`CLAUDE.md`, Demo mode). A demo detail never becomes a production default: the poll and flow TTL are asked of the source.
- **Traffic through `FlowIndex`**, never baked into the scan graph (`the_scan_graph_carries_no_traffic_of_its_own`).
- **Something changes every scan**, or update the live e2e test `applies live patches as the demo graph churns`, which waits for it.

## Follow-ups

### F1. A scripted story instead of a blinking record

Replace the canary toggle with a loop, one step per scan, each showing one capability with projector-produced shapes:

1. web-04 launches (instance, interface, address) and is registered in the ALB's target group → an `inferred` `Serves` edge appears.
2. Traffic reaches web-04 → `confirmed`.
3. web-01's traffic stops → it drops back to `inferred`, the transition `Serves` exists to express.
4. Azure is unreadable (`Unavailable`) → its estate is carried forward, nothing disappears, `/collection.json` names the failure.
5. Azure recovers; web-04 is terminated → its edges go.

Done when each step is visible within one poll at the default 5 s, the `Serves` counts move, and step 4 is visible somewhere a viewer looks (F5).

### F2. Make per-network addresses legible

Partly done by network connectivity phase 1: the staging VPC shows one value as two nodes, and peering and the transit gateway show resolution across joins. What remains are the two interconnect flows, honest but reading as broken until connectivity phase 2 joins AWS to Azure and GCP (`network_connectivity_design.md`); update this row then.

### F3. Filler that pushes the story off-screen

The 13 edgeless nodes (10 Azure, 3 GCP) and the 8 region-only AWS leaves exist for the exhaustiveness guard. On screen they scatter to the edges, and the camera frames them, so the connected estate is a small clump in the middle. Part of this is the projectors: GCP's DNS zone, function and bucket skip the project edge GCE instances get, and Azure has no resource-group or subscription kind to contain anything. Done when the demo graph reads as a few components and the camera frames the story.

### F4. Labels

Nodes are labelled with their full `Display` string (Azure ARM ids, `Generic::PrivateIpAddress(10.20.0.5@projects/…/networks/vpc-globex)`), so what renders overlaps and what doesn't is unknowable without tooltips. A short label (the last id segment, the address) with the full key on hover; part of rendering phase 3.

### F5. Collection health in the UI

`/collection.json` is the only place "could not read" is distinguishable from "is gone", and nothing in the UI shows it. Without a status panel or badge, F1's outage step is invisible.

### F6. Tier 1 in the demo (open)

Whether to simulate the event feed, e.g. a `RunInstances` that lands between scans. Weigh it against F1, which already shows topology change.
