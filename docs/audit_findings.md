# Codebase Audit & Cleanup Suggestions

*This document captures the findings of a deep codebase audit across the cloud-atlas project. All items below have been resolved (July 2026); it is kept as a record of the patterns to avoid reintroducing.*

## ✅ Resolved

### Bug Fixes
- **B1. GCP PubSub / Cloud Run / Storage missing auth headers** — fixed; all GCP API modules now go through `GoogleApiClient::get` (which adds `bearer_auth`) via the shared `paginated_list` helper.
- **B2. Azure NIC IDs stored as `AzureNetworkSecurityGroup` nodes** — fixed; `projector/azure.rs` uses `Node::AzureNetworkInterface`.
- **B3. Azure ignored user-provided subscriptions** — fixed; `build_azure` passes `opts.azure_subscriptions` to the ARG query (empty list = whole tenant).

### Consolidation
- **H1. AWS SDK config** — `cloud/amazon.rs` has a shared `load_config(region)`; `provider.rs` loads it once per region and passes `&SdkConfig` to every collector (also avoids 15 redundant credential-chain resolutions per region).
- **H2. GCP pagination** — `GoogleApiClient::paginated_list` handles URL/token/error/parse for every list endpoint; this also added previously missing pagination to pubsub, run, storage.
- **H3. Cloudflare HTTP** — `cloud/cloudflare/mod.rs` has a shared `api_get` that unwraps the `{ success, result }` envelope; the provider reuses a single `reqwest::Client`.
- **H4. tests.rs dead weight** — 360-line commented-out debug dump removed.
- **H5. Trivial GCP collector wrappers** — removed; `cloud/google/provider.rs` calls the `api/google` functions directly.

### Cleanup
- **M1. Dead code in `definition.rs`** — unused `Provider` enum, `Node::{AwsGlobal, AwsEventbridgeBus, AwsElbListener, AwsElbTargetHealth}`, and `Edge::Manages` removed.
- **M2. Leaf-node projector arms** — `project_leaf!` macro in `projector/{azure,gcp}.rs`.
- **M3. Unnecessary clones** — pagination-loop clones eliminated with the helper rewrites; `.as_str().into()` used over `.clone().into()`.
- **M4. Duplicate edges** — `GraphBuilder::add_edge` now skips identical existing edges.
- **M5. Test boilerplate** — `Settings` derives `Default`; tests use struct-update syntax; `assert_edge` uses `Iterator::any`.
- Low-impact polish: `DnsContent` import hoisted and A/AAAA/CNAME arms merged in `projector/cloudflare.rs`; `name` → `self_link` renames in `projector/gcp.rs`; `demo.rs` exports with `Display` like the engine; `build_cloudflare` returns `Provider::Cloudflare(...)`.
