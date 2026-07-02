# Cross-Cloud Network Inference and Telemetry

## Overview
Cloud Atlas aims to be a continuous, live digital twin of multi-cloud environments. Currently, resources are mapped independently per cloud. This design outlines how we will:
1. Discover cross-cloud dependencies via IP overlap.
2. Infer external and unsupported services via security configurations.
3. Support future lightweight liveness and network telemetry via event-driven flow log ingestion.

## 1. Cross-Cloud Connections via IP Overlap
We use the property graph model to pivot on `GenericIpAddress` nodes. 
- **Ingestion**: Network flow logs or routing tables provide source/destination IPs.
- **Mapping**: `[Provider A Resource] -> ConnectsTo -> GenericIpAddress(IP) <- HasIp <- [Provider B Resource]`.
- **Outcome**: Disparate clouds naturally connect in the unified graph through shared IP nodes.

## 2. Inferring External Services
For services without API support, we parse security boundaries (Security Groups, Firewalls) and DNS routing.
- **Egress Rules**: Map outbound CIDR blocks in Security Groups to `GenericIpAddress` nodes.
- **Domain Routing**: Map DNS records (e.g. Route53) to `GenericHostname` nodes.
- **Resolution**: Cross-reference known CIDR blocks to label IPs with `ExternalService(Name)` nodes.
- **Graph path**: `AwsEc2SecurityGroup -> RoutesTo -> GenericIpAddress -> ResolvesTo -> ExternalService`.

## 3. Lightweight Network Telemetry
Future live flow metrics (packet counts, blocked status) will be stored as properties (weights) on the edges.
- **Edge Definition**: Extend `Edge` (e.g., adding a `TrafficFlow` variant) to store `status`, `packet_count`, `last_seen`.
- **Ingestion**: Avoid polling. Use cloud-native streaming (AWS EventBridge/Kinesis, GCP Log Router -> Pub/Sub) for flow logs.
- **Performance**: In-memory graph edge updates are exceptionally fast, decoupling cloud log ingestion from graph liveness state.
