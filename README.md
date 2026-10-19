# Cloud Atlas

Discovers cloud configuration and maintains a live, in-memory property graph of multi-cloud infrastructure—a digital twin synchronized with deployed reality for architecture exploration and triage.

## Architecture

- **Typed graph:** 74 `Node` kinds (`AwsEc2Instance`, `AzureVirtualNetwork`, …) over `Arc<str>` IDs and 10 semantic `Edge` kinds (`AttachedTo`, `HasIp`, `RoutesTo`, …), stored in memory with `petgraph`.
- **`AtlasEngine`:** Concurrent multi-cloud collection, structured failure reporting, and persistent graph state.
- **Live backend (`atlas-server/`):** Reconciles against providers, diffs by stable resource key, and streams patches over WebSocket across three change tiers: control-plane event streams (Tier 1), flow-log liveness overlays (Tier 2), and full-scan polling (Tier 3). See [`docs/change_monitoring_design.md`](docs/change_monitoring_design.md).
- **Liveness overlay:** Observed traffic lives beside the graph in a bounded, expiring `FlowIndex`. `Edge::TrafficFlow` asserts only that traffic occurred; counters and timestamps ride the snapshot's `observations` list.
- **Service topology:** Derived `Edge::Serves` links collapse load balancer → interface → address → instance paths into direct resource edges, `inferred` from control-plane wiring and `confirmed` by traffic. See [`docs/service_topology_design.md`](docs/service_topology_design.md).
- **Interactive rendering (`atlas-render/`):** Rust/WebAssembly ForceAtlas2 layout feeding a Sigma.js WebGL frontend. See [`atlas-render/README.md`](atlas-render/README.md).

Shared pivots (`GenericHostname`, `GenericIpAddress`) stitch clouds together (e.g. Route 53 resolving to Azure App Service or GCP Cloud Run). Private addresses scope to their network (`GenericPrivateIpAddress`) and resolve across modelled peerings and transit gateways (`docs/network_connectivity_design.md`). DNS across all providers projects `name -ResolvesTo-> record -ResolvesTo-> target`.

## Status & Roadmap

- [x] Live, in-memory graph exported to `.dot` ([Gephi](https://gephi.org/)) and WebGL
- [x] Multi-cloud collection: AWS, GCP, Azure, and Cloudflare
- [x] Persistent graph + differ streaming patches over WebSocket
- [ ] Tier-1 control-plane event streams
  - [x] AWS — EventBridge / Config / CloudTrail over SQS (`--aws-event-queue`)
  - [ ] GCP Cloud Asset Inventory feeds, Azure Event Grid (Cloudflare remains on polling)
- [ ] Tier-2 data-plane liveness overlay
  - [x] AWS VPC Flow Logs → S3 → SQS (`--aws-flow-log-queue`)
  - [ ] GCP and Azure flow logs
- [ ] Cross-cloud VPNs, Direct Connect, ExpressRoute, and Cloud Interconnect
- [ ] On-prem discovery

## Providers

- **AWS** (`--regions`, standard credential chain): EC2, VPC, ENIs, Peering, Transit Gateways, ELB, Route 53, S3, RDS, DynamoDB, Lambda, ECS, EKS, API Gateway, SQS, SNS, CloudFront, IAM, Config. S3 and Route 53 collect under `global`.
- **GCP** (`--gcp-projects`, gcloud ADC or browser OAuth): Compute, Firewalls, Forwarding Rules, VPC Networks/Subnetworks/Peerings, Cloud SQL, Cloud DNS, GKE, Cloud Functions, Pub/Sub, Cloud Run, Cloud Storage.
- **Cloudflare** (`--cloudflare`, `CLOUDFLARE_API_TOKEN`): Zones, DNS records, Workers, Durable Objects, KV, R2, D1.
- **Azure** (`--azure-subscriptions`, `az login`, Resource Graph): VMs, AKS, App Services, Function Apps, API Management, VNets/Subnets/Peerings, NICs, Load Balancers, NSGs, Public IPs, DNS Zones, CDN, Storage, SQL, Cosmos DB, Service Bus, Event Grid.

## CLI (`atlas`)

`cargo build --release` builds `target/release/atlas` and writes `atlas.dot` and `atlas.json`.

```bash
cargo run --bin atlas                                      # AWS us-east-1
cargo run --bin atlas -- --regions us-east-1 us-west-2
cargo run --bin atlas -- --gcp-projects proj-1 proj-2
CLOUDFLARE_API_TOKEN=… cargo run --bin atlas -- --cloudflare
cargo run --bin atlas -- --azure-subscriptions sub-1 sub-2
cargo run --bin atlas -- --daemon --verbose                # re-scan every 60s
```

In `--daemon` mode, unreadable sources are carried forward for a bounded number of scans rather than diffed as mass deletions.

## Live Stack (`atlas-server` + `atlas-render`)

```bash
cargo xtask dev --demo    # credential-free: wasm → atlas-server :4681 → frontend :4680
cargo xtask dev           # real collection; pass provider flags
cargo xtask test [--e2e]  # all test suites in order
```

With live AWS feeds:

```bash
cargo run -p atlas-server -- --regions us-east-1 \
  --aws-event-queue    https://sqs.us-east-1.amazonaws.com/111/atlas-events \
  --aws-flow-log-queue https://sqs.us-east-1.amazonaws.com/111/atlas-flows
```

See [`atlas-server/README.md`](atlas-server/README.md) and [`atlas-render/README.md`](atlas-render/README.md).
