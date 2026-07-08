# Cloud Atlas

A project that discovers cloud configurations and maintains a live, in-memory property graph of the infrastructure.
The goal is to make it easy to gain fast visual insight into how infrastructure is configured and connected by keeping a digital twin of the environment continuously synchronized with real-world reality.
This is intended to be a visual aide to help with discussions involving architecture, and triage.
It builds from existing cloud configurations as they exist in reality, not an idealized view of intent.

## Architecture

Cloud Atlas builds a **Strongly Typed Semantic Graph**:
- **Nodes**: Modeled as specific Enum variants for nearly 70 cloud resources (e.g., `Node::AwsEc2Instance`, `Node::AzureVirtualNetwork`), wrapping zero-copy `Arc<str>` types for high-performance memory efficiency.
- **Edges**: Relationships go beyond simple containment, leveraging strict semantic edges like `AttachedTo`, `HasIp`, and `RoutesTo` to deeply mimic network topology. 
- **Graph Storage**: The graph is stored entirely in memory using `petgraph`, enabling extremely fast deduplication and continuous traversal.
- **Core Orchestration**: Driven by the `AtlasEngine`, which handles concurrent fetching, graceful error handling, and long-living graph state management for continuous daemon loops.
- **Live Backend** (`atlas-server/`): a long-running server that owns a persistent copy of the graph, reconciles it against the providers on an interval, diffs each scan by a stable per-resource key, and pushes the resulting add/remove patches to connected frontends over WebSocket. See [`docs/change_monitoring_design.md`](docs/change_monitoring_design.md).
- **Interactive Rendering** (`atlas-render/`): a separate cargo + bun workspace — a WebAssembly force-directed layout engine feeding a Sigma.js WebGL frontend, fed live by `atlas-server`. See [`docs/graph_rendering_design.md`](docs/graph_rendering_design.md) and [`atlas-render/README.md`](atlas-render/README.md).

## Goals

- [x] Maintain a live, in-memory graph continuously synchronized
- [x] Visualize graph in comprehensible layout exploring network/service layers
- [x] Make the graph explorable
    - [x] Outputs a point-in-time `dot` file (explorable w/ other tools like `gephi`)
    - [x] Interactive, live-updating WebGL rendering (`atlas-render/`)
- [ ] Work across GCP, AWS, Azure, and Cloudflare
    - [x] AWS
    - [x] GCP
    - [x] Cloudflare
    - [x] Azure
- [ ] Push incremental updates instead of full-graph rescans
    - [x] Persistent graph + differ (`atlas_lib::atlas::patch::diff`)
    - [x] Live server pushing patches to the frontend over WebSocket
    - [ ] Cloud-native event/audit streams as the primary change feed (today it's diffed full-scan polling)
- [ ] Extendable for on-prem use-cases

### Status

The best tool I know of for exploring the dot file so far has been [gephi](https://gephi.org/); the interactive renderer in `atlas-render/` is now the faster path for live exploration, especially when backed by `atlas-server`.
The graph can now fetch resources concurrently across multiple AWS regions, GCP projects, and Azure subscriptions, merging them into a single comprehensive in-memory model. Thanks to shared pivot nodes (`GenericHostname` and `GenericIpAddress`), Cloud Atlas natively visualizes cross-cloud connectivity (e.g. AWS Route53 routing directly to Azure App Services or GCP Cloud Run).
Change detection is currently diffed full-scan polling; cloud-native event streams (EventBridge, Cloud Asset Inventory, Event Grid) are the next step toward sub-minute latency — see the phased plan in `docs/change_monitoring_design.md`.

## AWS Notes

The global region is for resources that don't cleanly map to a specific region.

### S3
S3 Buckets are not region specific so the relationships for a bucket should point to all resources in all regions.

### Route53
Route53 Hosted Zones and Record Sets are mapped globally. Record Sets project `ConnectsTo` edges directly to the IPs and Alias Targets (like Load Balancers) they route traffic to.

## GCP Notes

GCP resources are supported using lightweight custom REST clients for performance and reduced binary bloat. Authenticate locally and use the `--gcp-projects` flag to include GCP resources in the final graph output. Supported services include Compute Instances, Firewalls, Cloud SQL, Cloud DNS, GKE, Cloud Functions, Pub/Sub, Cloud Run, and Network topologies.

## Cloudflare Notes

Cloudflare resources are supported using the `CLOUDFLARE_API_TOKEN` environment variable for authentication. Use the `--cloudflare` flag to include Cloudflare resources in the final graph output. Supported services include Zones, DNS Records, Workers, Durable Objects, KV Namespaces, R2 Buckets, and D1 Databases.

## Azure Notes

Azure resources are supported using Azure Resource Graph (ARG) for blazing fast, cross-subscription resource fetching. Authenticate locally with `az login` and use the `--azure-subscriptions` flag to include Azure resources in the final graph output. Supported services now include a wide range of managed services:
- **Compute**: Virtual Machines, AKS (Managed Clusters), App Services, Function Apps
- **Network**: Virtual Networks, Subnets, NSGs, Public IPs, DNS Zones, CDN Profiles
- **Storage/Database**: Storage Accounts, SQL Servers, Cosmos DB
- **Messaging**: Service Bus, Event Grid

## Build Instructions

Nothing fancy right now, a simple `cargo build --release` will generate a binary named `atlas`.
This is a simple CLI utility.

## Running the CLI (one-shot / batch)

Using `atlas` assumes that AWS credentials are in place. For GCP, it will use your local gcloud authentication or pop a browser window for OAuth 2.0 Installed Flow.
It runs and generates an `atlas.dot` file (plus a render snapshot `atlas.json`) in the directory being run.

```bash
# Run a single point-in-time snapshot for AWS (us-east-1)
cargo run

# Run a snapshot for multiple AWS regions concurrently
cargo run -- --regions us-east-1 us-west-2

# Include GCP projects in the snapshot
cargo run -- --regions us-east-1 --gcp-projects my-gcp-project-1 my-gcp-project-2

# Include Cloudflare resources in the snapshot
CLOUDFLARE_API_TOKEN=your_token_here cargo run -- --cloudflare

# Include Azure subscriptions in the snapshot
cargo run -- --regions us-east-1 --azure-subscriptions my-subscription-1 my-subscription-2

# Run as a continuously updating daemon (polls every 60s)
cargo run -- --daemon

# Include all default mappings and enable verbose output
cargo run -- --all --verbose
```

## Running the Live Stack (server + renderer)

For a live, continuously-updating view instead of a one-shot snapshot, use
[`atlas-server`](atlas-server/README.md) together with the
[`atlas-render`](atlas-render/README.md) frontend. `cargo xtask` is the unified
entry point for running (and testing) all of it together:

```bash
cargo xtask dev --demo    # whole stack, credential-free: wasm renderer (rebuilt if
                          #   stale) → atlas-server on :4681 → frontend on :4680.
                          #   Ctrl-C stops everything.
cargo xtask dev           # same, real collection (default; pass provider flags,
                          #   e.g. --regions us-east-1 --cloudflare, as needed)
cargo xtask test [--e2e]  # every test suite across the whole repo, in order
```

`atlas-server` is the same provider collection as the CLI, but long-running: it
never wipes its graph, diffs each reconciliation scan, and pushes incremental
patches to the frontend over WebSocket instead of writing a static file. See
[`atlas-server/README.md`](atlas-server/README.md) for the standalone server and
[`atlas-render/README.md`](atlas-render/README.md) for the rendering stack;
`CLAUDE.md`'s "Dev Orchestration" section has the full `cargo xtask` reference.
