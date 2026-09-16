# Cloud Atlas

Discovers cloud configuration and keeps a live, in-memory property graph of the infrastructure — a digital twin continuously synchronized with reality, built from what is actually deployed rather than an idealized view of intent. Intended as a visual aid for architecture discussions and triage.

## Architecture

- **Typed graph**: 70 `Node` variants (`Node::AwsEc2Instance`, `Node::AzureVirtualNetwork`, …) over `Arc<str>` ids, and semantic edges (`AttachedTo`, `HasIp`, `RoutesTo`, …), stored in memory with `petgraph`.
- **`AtlasEngine`**: concurrent collection, failure reporting, and long-lived graph state.
- **Live backend** (`atlas-server/`): owns a persistent graph, diffs each scan by stable resource key, and pushes add/remove patches over WebSocket. Change detection has three tiers — cloud event streams (1), flow logs as a liveness overlay (2), and full-scan polling as the backstop (3). See [`docs/change_monitoring_design.md`](docs/change_monitoring_design.md).
- **Liveness overlay**: observed traffic lives *beside* the graph in a bounded, expiring `FlowIndex`. `Edge::TrafficFlow` only says "traffic was seen"; counters and timestamps travel on the snapshot's `observations` list.
- **Service topology**: derived `Edge::Serves` links collapse load balancer → interface → address → instance paths into one edge, `inferred` from wiring and `confirmed` by traffic. See [`docs/service_topology_design.md`](docs/service_topology_design.md).
- **Interactive rendering** (`atlas-render/`): a WebAssembly force-directed layout feeding a Sigma.js WebGL frontend. See [`atlas-render/README.md`](atlas-render/README.md).

Shared pivots (`GenericHostname`, `GenericIpAddress`) stitch clouds together, e.g. Route 53 resolving to an Azure App Service or GCP Cloud Run.

## Goals

- [x] Live, in-memory graph kept continuously in sync
- [x] Explorable: `.dot` export (e.g. [Gephi](https://gephi.org/)) and a live WebGL renderer
- [x] Collect from AWS, GCP, Azure and Cloudflare
- [x] Persistent graph + differ, patches pushed over WebSocket
- [ ] Cloud event streams as the primary change feed
    - [x] AWS — EventBridge/Config/CloudTrail over SQS (`--aws-event-queue`)
    - [ ] GCP asset feeds, Azure Event Grid (Cloudflare stays on polling)
- [ ] Data-plane liveness overlay
    - [x] AWS VPC Flow Logs → S3 → SQS (`--aws-flow-log-queue`)
    - [ ] GCP and Azure flow logs
- [ ] On-prem support

## Providers

- **AWS** — standard credential chain; `--regions`. Resources that don't map to a region live under a `global` scope. S3 buckets are global. Route 53 zones and record sets are global, and record sets `ResolvesTo` their IPs and alias targets.
- **GCP** — lightweight REST clients; local gcloud auth or a browser OAuth flow; `--gcp-projects`. Compute, firewalls, Cloud SQL, Cloud DNS, GKE, Cloud Functions, Pub/Sub, Cloud Run, networking.
- **Cloudflare** — `CLOUDFLARE_API_TOKEN`; `--cloudflare`. Zones, DNS records, Workers, Durable Objects, KV, R2, D1.
- **Azure** — Azure Resource Graph, cross-subscription; `az login`; `--azure-subscriptions`. VMs, AKS, App Services, Function Apps, VNets, subnets, NSGs, public IPs, DNS zones, CDN profiles, Storage, SQL, Cosmos DB, Service Bus, Event Grid.

## CLI (one-shot / daemon)

`cargo build --release` produces `target/release/atlas`. Each run writes `atlas.dot` and a render snapshot `atlas.json` to the working directory.

```bash
cargo run --bin atlas                                      # AWS us-east-1
cargo run --bin atlas -- --regions us-east-1 us-west-2
cargo run --bin atlas -- --gcp-projects proj-1 proj-2
CLOUDFLARE_API_TOKEN=… cargo run --bin atlas -- --cloudflare
cargo run --bin atlas -- --azure-subscriptions sub-1 sub-2
cargo run --bin atlas -- --daemon                          # re-scan every 60s
cargo run --bin atlas -- --verbose
```

The daemon never wipes its graph: each tick is diffed against the last, and a provider that could not be read is carried forward for a bounded number of scans instead of looking like a mass deletion.

## Live stack (server + renderer)

```bash
cargo xtask dev --demo    # credential-free: wasm → atlas-server :4681 → frontend :4680
cargo xtask dev           # real collection; pass provider flags
cargo xtask test [--e2e]  # every test suite, in order
```

`atlas-server` also consumes the live AWS feeds:

```bash
cargo run -p atlas-server -- --regions us-east-1 \
  --aws-event-queue    https://sqs.us-east-1.amazonaws.com/111/atlas-events \
  --aws-flow-log-queue https://sqs.us-east-1.amazonaws.com/111/atlas-flows
```

See [`atlas-server/README.md`](atlas-server/README.md) and [`atlas-render/README.md`](atlas-render/README.md).
