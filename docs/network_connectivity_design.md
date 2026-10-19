# Network Connectivity: Networks, Peering, Transit Gateways & Interconnects

Private addresses are keyed per network (`GenericPrivateIpAddress { network, address }`, [`docs/network_inference_design.md`](network_inference_design.md) §1), keeping overlapping RFC 1918 ranges separate while requiring explicit network joins to resolve cross-network traffic. Node identity remains per network; cross-network resolution happens at derive time. Historical decisions live in [`docs/audit_findings.md`](audit_findings.md).

## 1. Network Identity

`AwsEc2Vpc`, `AzureVirtualNetwork`, and `GcpComputeNetwork` represent networks. Every network node's ID equals the `network` scope of its private addresses (`Node::network_scope()`):

| Provider | Network ID & Scope |
|---|---|
| **AWS** | VPC ID (`DescribeVpcs` places every VPC under its region, including empty and peer-only VPCs) |
| **Azure** | Lowercased VNet ARM ID (`canonical_azure_id`); NIC addresses derive their VNet from `subnet.id` |
| **GCP** | `projects/P/global/networks/N` via `canonical_network` (normalising full URLs, relative paths, and bare names like GKE's `network`; `networkConfig.network` takes precedence on GKE) |

## 2. Modelled Joins

Joins project as standard graph edges using existing `Edge` kinds:

| Provider | Collected | Projected |
|---|---|---|
| **AWS** | `DescribeVpcPeeringConnections` | `AwsEc2VpcPeeringConnection -AttachedTo->` requester and accepter VPCs (`active` only) |
| **AWS** | `DescribeTransitGateways`, `DescribeTransitGatewayAttachments` | `Region -Contains-> AwsEc2TransitGateway` (`available`/`modifying`); `-AttachedTo->` each VPC with an `available`/`modifying` VPC attachment |
| **AWS** | Route tables | `RouteTable -RoutesTo->` peering connection or transit gateway for non-`blackhole` routes |
| **Azure** | VNet `virtualNetworkPeerings` | `AzureVirtualNetwork -ConnectsTo-> AzureVirtualNetwork` (`Connected` only) |
| **GCP** | `networks.list` `peerings` | `GcpComputeNetwork -ConnectsTo-> GcpComputeNetwork` (`ACTIVE` only) |

- **Filtering & lifecycle:** Blackhole routes (including NAT/IGW), rejected/deleted peerings, and pending attachments project nothing. `modifying` gateways and attachments stay projected so active paths do not flap during changes. Cross-account peer VPCs project as bare `AwsEc2Vpc` nodes.
- **AWS Config exclusions:** `AWS::EC2::VPCPeeringConnection`, `TransitGateway`, and `TransitGatewayAttachment` are excluded in `use_aws_resource` (like NAT, EIP, and ENI). `AWS::EC2::VPC` remains in the Config catch-all alongside its `AwsConfigResource` twin.
- **IAM:** Requires `ec2:DescribeVpcs`, `ec2:DescribeVpcPeeringConnections`, `ec2:DescribeTransitGateways`, and `ec2:DescribeTransitGatewayAttachments` (all included in `ReadOnlyAccess`).

## 3. Cross-Network Address Resolution

`derive/networks.rs` defines `JOINS` (validated against `ALL_KINDS`), matching projected edges in either direction:

```
Through { network: AwsEc2Vpc, connector: AwsEc2VpcPeeringConnection }
Through { network: AwsEc2Vpc, connector: AwsEc2TransitGateway }
Direct  { network: AzureVirtualNetwork }
Direct  { network: GcpComputeNetwork }
```

In `service::address_owners`, an unheld private pivot `a@N` inherits the owners of `a@M` when **at most one—and exactly one—network `M` joined to `N` holds `a`**:

- **Local holder wins:** An address held in `N` never looks across a join.
- **Non-transitive:** `A–B` and `B–C` never join `A` to `C`. Transit gateways act as hubs joining all attached VPCs.
- **Ambiguity resolves to nothing:** If two joined networks hold `a`, neither wins.
- **Stopped-instance gate:** `link_targets` queries `derive::held_addresses` so IP targets resolving locally or across a join to an unscanned instance's ENI are skipped.
- **Hub representation:** Connectors stay hubs ($O(k)$ attachments, never expanded into $k^2$ pairs).
- **Single integration point:** Sitting inside `address_owners`, resolution automatically feeds observed flows, IP-target chains (`NLB` targeting a peered VPC IP → `inferred`), and the service view.

Pinned by `a_private_address_resolves_across_a_peering`, `a_private_address_resolves_across_a_transit_gateway`, `peering_does_not_chain`, `an_address_two_joined_networks_hold_resolves_to_neither`, `a_holder_in_the_reading_network_wins_over_any_joined_one`, `an_ip_target_in_a_peered_vpc_is_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred`, and property test `an_unheld_address_resolves_through_exactly_one_joined_holder`.

### Known Gaps (Addressed in Phase 3)

- **TGW overlap:** Without VPC CIDR ranges, an unheld address inside the reading VPC's own CIDR can resolve across a TGW to an overlapping attached VPC.
- **TGW route-table segmentation:** All VPC attachments on a TGW currently join each other regardless of per-attachment TGW route tables, so traffic attempted toward an isolated attachment's IP (logged as accepted on the source ENI before the TGW drops it) resolves to that holder.

## 4. Globex Fixture Coverage

- **AWS peering:** `vpc-globex` ↔ `vpc-globex-data` via `pcx-globex-data` (`i-globex-warehouse` at `10.40.1.20`; flow from `web-01` derives `web-01 -Serves-> i-globex-warehouse`).
- **AWS TGW:** `tgw-globex` joins `vpc-globex` and `vpc-globex-shared` (`eni-globex-ldap` at `10.50.0.10`; flow from `web-02` derives `web-02 -Serves-> AwsEc2Eni(eni-globex-ldap)`). Prod's private route table routes to both `pcx-globex-data` and `tgw-globex`.
- **Rejected peering & overlap:** `vpc-globex-staging` reuses `10.10.0.0/16` (`10.10.1.10`) with a rejected peering request—isolated from prod.
- **Azure & GCP:** `vnet-globex-hub` ↔ `vnet-globex` (`Connected`), plus a one-sided `vnet-globex-dr` peering (ignored); `vpc-globex` ↔ Cloud SQL producer network (`ACTIVE`), plus an `INACTIVE` peering (ignored).

## 5. Remaining Phases

| Phase | Deliverable | Status |
|---|---|---|
| **2** | **Interconnects & VPNs:** AWS VPN (VGW, CGW, VPN connections, TGW VPN attachments) and Direct Connect (`aws-sdk-directconnect`); Azure virtual/local network gateways, connections, ExpressRoute circuits, and gateway transit (`useRemoteGateways`); GCP HA VPN gateways, tunnels, external VPN gateways, Cloud Routers, and interconnect attachments. Cross-cloud joins stitch through shared public tunnel-endpoint pivots via `RoutesTo` (never `ConnectsTo`), adding a two-gateway tunnel `Join` form and resolving the fixture's AWS → Azure VM and AWS → Cloud SQL flows. | Next |
| **3** | **Address spaces & routing precision:** Project VPC CIDRs, VNet prefixes, and GCP subnet ranges without flooding `Covers`; attribute unheld local addresses to covering networks; enforce VPC and TGW route tables during resolution; add Tier-1 mappings for connectivity kinds, Virtual WAN / NCC hubs, and TGW peering. | Later |
