# Cloud Atlas

A project that that reads cloud configurations and outputs graph data.
The goal is to make it easy to gain fast visual insight into how infrastructure is configured and connected.
This is intended to be a visual aide to help with discussions involving architecture, and triage.
It builds from existing cloud configurations as they exist in reality, not an idealized view of intent.

## Goals

- [ ] Visualize graph in comprehensible layout exploring network/service layers
- [ ] Make the graph explorable
    - [x] Outputs a `dot` and explorable w/ other tools like `gephi`
- [ ] Work across GCP, AWS, and Azure
    - [x] AWS
- [ ] Extendable for on-prem use-cases

### Status

The best tool I know of for exploring the dot file so far has been [gephi](https://gephi.org/).
Current work is being done to build better relationships in the graph output.
The graph only generates for a single AWS region, but it is intended to give multi-region relationships eventually.

## AWS Notes

The global region is for resources that don't cleanly map to a specific region.

### S3

S3 Buckets are not region specific so the relationships for a bucket should point to all resources in all regions.

## Build Instructions

Nothing fancy right now, a simple `cargo build --release` will generate a binary named `atlas`.
This is a simple CLI utility.

## Running

Using `atlas` assumes that AWS credentials are in place.
It runs and generates an `atlas.dot` file in the directory being run.
