# Cloud Atlas

A project that discovers cloud configurations and maintains a live, in-memory property graph of the infrastructure.
The goal is to make it easy to gain fast visual insight into how infrastructure is configured and connected by keeping a digital twin of the environment continuously synchronized with real-world reality.
This is intended to be a visual aide to help with discussions involving architecture, and triage.
It builds from existing cloud configurations as they exist in reality, not an idealized view of intent.

## Architecture

Cloud Atlas builds a **Strongly Typed Semantic Graph**:
- **Nodes**: Modeled as specific Enum variants for over 40 cloud resources (e.g., `Node::AwsEc2Instance`, `Node::AzureVirtualNetwork`), wrapping zero-copy `Arc<str>` types for high-performance memory efficiency.
- **Edges**: Relationships go beyond simple containment, leveraging strict semantic edges like `AttachedTo`, `HasIp`, and `RoutesTo` to deeply mimic network topology. 
- **Graph Storage**: The graph is stored entirely in memory using `petgraph`, enabling extremely fast deduplication and continuous traversal.
- **Core Orchestration**: Driven by the `AtlasEngine`, which handles concurrent fetching, graceful error handling, and long-living graph state management for continuous daemon loops.

## Goals

- [x] Maintain a live, in-memory graph continuously synchronized
- [ ] Visualize graph in comprehensible layout exploring network/service layers
- [ ] Make the graph explorable
    - [x] Outputs a point-in-time `dot` file (explorable w/ other tools like `gephi`)
- [ ] Work across GCP, AWS, and Azure
    - [x] AWS
    - [x] GCP
    - [x] Azure
- [ ] Extendable for on-prem use-cases

### Status

The best tool I know of for exploring the dot file so far has been [gephi](https://gephi.org/).
Current work is being done to build better relationships in the graph output.
The graph can now fetch resources concurrently across multiple AWS regions, GCP projects, and Azure subscriptions, merging them into a single comprehensive in-memory model. Thanks to shared pivot nodes (`GenericHostname` and `GenericIpAddress`), Cloud Atlas natively visualizes cross-cloud connectivity (e.g. AWS Route53 routing directly to Azure App Services or GCP Cloud Run).

## AWS Notes

The global region is for resources that don't cleanly map to a specific region.

### S3
S3 Buckets are not region specific so the relationships for a bucket should point to all resources in all regions.

### Route53
Route53 Hosted Zones and Record Sets are mapped globally. Record Sets project `ConnectsTo` edges directly to the IPs and Alias Targets (like Load Balancers) they route traffic to.

## GCP Notes

GCP resources are supported using lightweight custom REST clients for performance and reduced binary bloat. Authenticate locally and use the `--gcp-projects` flag to include GCP resources in the final graph output. Supported services include Compute Instances, Firewalls, Cloud SQL, Cloud DNS, GKE, Cloud Functions, Pub/Sub, Cloud Run, and Network topologies.

## Azure Notes

Azure resources are supported using Azure Resource Graph (ARG) for blazing fast, cross-subscription resource fetching. Authenticate locally with `az login` and use the `--azure-subscriptions` flag to include Azure resources in the final graph output. Supported services now include a wide range of managed services:
- **Compute**: Virtual Machines, AKS (Managed Clusters), App Services, Function Apps
- **Network**: Virtual Networks, Subnets, NSGs, Public IPs, DNS Zones, CDN Profiles
- **Storage/Database**: Storage Accounts, SQL Servers, Cosmos DB
- **Messaging**: Service Bus, Event Grid

## Build Instructions

Nothing fancy right now, a simple `cargo build --release` will generate a binary named `atlas`.
This is a simple CLI utility.

## Running

Using `atlas` assumes that AWS credentials are in place. For GCP, it will use your local gcloud authentication or pop a browser window for OAuth 2.0 Installed Flow.
It runs and generates an `atlas.dot` file in the directory being run.

```bash
# Run a single point-in-time snapshot for AWS (us-east-1)
cargo run

# Run a snapshot for multiple AWS regions concurrently
cargo run -- --regions us-east-1 us-west-2

# Include GCP projects in the snapshot
cargo run -- --regions us-east-1 --gcp-projects my-gcp-project-1 my-gcp-project-2

# Include Azure subscriptions in the snapshot
cargo run -- --regions us-east-1 --azure-subscriptions my-subscription-1 my-subscription-2

# Run as a continuously updating daemon (polls every 60s)
cargo run -- --daemon
```
