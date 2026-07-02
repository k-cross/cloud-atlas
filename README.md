# Cloud Atlas

A project that discovers cloud configurations and maintains a live, in-memory property graph of the infrastructure.
The goal is to make it easy to gain fast visual insight into how infrastructure is configured and connected by keeping a digital twin of the environment continuously synchronized with real-world reality.
This is intended to be a visual aide to help with discussions involving architecture, and triage.
It builds from existing cloud configurations as they exist in reality, not an idealized view of intent.

## Goals

- [x] Maintain a live, in-memory graph continuously synchronized
- [ ] Visualize graph in comprehensible layout exploring network/service layers
- [ ] Make the graph explorable
    - [x] Outputs a point-in-time `dot` file (explorable w/ other tools like `gephi`)
- [ ] Work across GCP, AWS, and Azure
    - [x] AWS
    - [x] GCP
- [ ] Extendable for on-prem use-cases

### Status

The best tool I know of for exploring the dot file so far has been [gephi](https://gephi.org/).
Current work is being done to build better relationships in the graph output.
The graph can now fetch resources concurrently across multiple AWS regions and GCP projects, merging them into a single comprehensive in-memory model.

## AWS Notes

The global region is for resources that don't cleanly map to a specific region.

### S3
S3 Buckets are not region specific so the relationships for a bucket should point to all resources in all regions.

### Route53
Route53 Hosted Zones and Record Sets are mapped globally. Record Sets project `ConnectsTo` edges directly to the IPs and Alias Targets (like Load Balancers) they route traffic to.

## GCP Notes

GCP resources are supported using lightweight custom REST clients for performance and reduced binary bloat. Authenticate locally and use the `--gcp-projects` flag to include GCP resources in the final graph output. Supported services include Compute Instances, Firewalls, Cloud SQL, Cloud DNS, GKE, and Cloud Functions.

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

# Run as a continuously updating daemon (polls every 60s)
cargo run -- --daemon
```
