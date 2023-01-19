# Cloud Atlas

A project that that reads cloud configurations and outputs graph data.
The goal is to make it easy to gain fast visual insight into how infrastructure is configured and connected.
This is intended to be a visual aide to help with discussions involving architecture, and triage.
It builds from existing cloud configurations as they exist in reality, not an idealized view of intent.

## Goals

[ ] Visualize graph in comprehensible layout
[ ] Make the graph explorable
[ ] Work across GCP, AWS, and Azure
[ ] Extendable for on-prem use-cases

### Status

[x] Works with AWS
[x] Outputs a `dot`

The best tool for exploring the dot file so far has been [gephi](https://gephi.org/).
Current work is being done to build better relationships in the graph output.
The graph only generates for a single AWS region, but it is intended to give multi-region relationships eventually.
