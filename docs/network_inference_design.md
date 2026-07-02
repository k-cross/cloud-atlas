# Cross-Cloud Network Inference and Telemetry

## Overview
Cloud Atlas aims to be a continuous, live digital twin of multi-cloud environments. Currently, resources are mapped independently per cloud. This design outlines how we will:
1. Discover cross-cloud dependencies via IP overlap.
2. Infer external and unsupported services via security configurations.
3. Support future lightweight liveness and network telemetry via event-driven flow log ingestion.

## 1. Cross-Cloud Connections via Universal Pivot Nodes
We use the property graph model to organically merge disparate cloud environments by pivoting on universal identifiers.
- **Node Types**: `Node::GenericIpAddress` and `Node::GenericHostname` act as the common language between clouds.
- **Mapping**: For instance, Cloudflare DNS connects via `Edge::ResolvesTo -> Node::GenericIpAddress`, while an AWS EC2 Instance connects via `Edge::ConnectsTo -> Node::GenericIpAddress`. 
- **Outcome**: The graph deduplicates identical nodes, seamlessly connecting Cloudflare and AWS without any direct API-level correlation.

## 2. Inferring External Services
For services without direct API integration, we infer their presence using security boundaries and DNS.
- **Security Boundaries**: We parse outbound rules in AWS Security Groups, GCP Firewalls, and Azure Network Security Groups, mapping explicit CIDRs to `GenericIpAddress` nodes via `Edge::RoutesTo`.
- **Cloud-Specific Abstractions**: Mappings that don't resolve to raw IPs (like Azure Service Tags) are given dedicated types (e.g. `Node::AzureServiceTag`) to prevent generic node pollution.
- **Graph path**: `AwsEc2SecurityGroup -> RoutesTo -> GenericIpAddress -> ResolvesTo -> ExternalService`.

## 3. Lightweight Network Telemetry
Future live flow metrics (packet counts, blocked status) will be stored as properties (weights) on the edges.
- **Edge Definition**: Extend `Edge` (e.g., adding a `TrafficFlow` variant) to store `status`, `packet_count`, `last_seen`.
- **Ingestion**: Avoid polling. Use cloud-native streaming (AWS EventBridge/Kinesis, GCP Log Router -> Pub/Sub) for flow logs.
- **Performance**: In-memory graph edge updates are exceptionally fast, decoupling cloud log ingestion from graph liveness state.
