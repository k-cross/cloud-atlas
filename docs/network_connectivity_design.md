# Network Connectivity: Networks, Peering, Transit Gateways and Interconnects

## Goal

Private addresses are identified per network (`GenericPrivateIpAddress { network, address }`, `network_inference_design.md` §1), so `10.0.1.5` in one VPC is never `10.0.1.5` in another. That stopped unrelated networks that reuse a range from being joined. It also cut the joins that are real: traffic over peering, a transit gateway or an interconnect carries the far side's private address, read in the near side's network, where nothing holds it. This document models the joins between networks and resolves a private address across them. Node identity stays per network.

## Networks

`AwsEc2Vpc`, `AzureVirtualNetwork` and `GcpComputeNetwork` are the networks. A network node's id is exactly the scope its private addresses are read in, so `Node::network_scope()` reads one off the other.

| Provider | Id = scope |
|---|---|
| AWS | VPC id; `DescribeVpcs` puts every VPC under its region, including empty ones and ones reached only by peering |
| Azure | VNet ARM id, lowercased (`canonical_azure_id`); a NIC's addresses take the VNet from their subnet's id |
| GCP | `projects/P/global/networks/N`, cut from whatever URL form the API returns, or qualified with the scanning project when it returns a bare name (GKE's `network`); `networkConfig.network` wins for GKE |

GCP used to key networks by raw API strings, so GKE's bare `default` and an instance's full URL were two nodes for one network.

## Joins (phase 1)

Joins are ordinary projected edges, and no new `Edge` kinds were needed:

| Provider | Collected | Projected |
|---|---|---|
| AWS | `DescribeVpcPeeringConnections` | `AwsEc2VpcPeeringConnection -AttachedTo->` requester and accepter VPCs, status `active` only |
| AWS | `DescribeTransitGateways`, `DescribeTransitGatewayAttachments` | `Region -Contains-> AwsEc2TransitGateway` (`available` or `modifying`); `-AttachedTo->` each VPC with an `available` or `modifying` vpc attachment |
| AWS | route tables (existing) | `RouteTable -RoutesTo->` the peering connection or transit gateway of each route that is not a blackhole |
| Azure | VNet row's `virtualNetworkPeerings` | `AzureVirtualNetwork -ConnectsTo-> AzureVirtualNetwork` for each `Connected` peering |
| GCP | `networks.list` `peerings` | `GcpComputeNetwork -ConnectsTo-> GcpComputeNetwork` for each `ACTIVE` peering |

The AWS shapes follow the IGW precedent (`gateway -AttachedTo-> Vpc`). A blackhole route (to any target, NAT and IGW included), a rejected or deleted peering, and a pending attachment draw nothing; a gateway or attachment mid-modification still carries traffic, so it stays drawn rather than flapping out of the graph for a scan. A peer VPC in another account becomes a bare VPC node: real, and outside the scan. The Config types `AWS::EC2::VPCPeeringConnection`, `TransitGateway` and `TransitGatewayAttachment` are excluded from the generic catch-all (`use_aws_resource`), as NAT, EIP and ENI already were, so none of them is both typed and generic. The VPC itself still is, like the instance and the subnet: `AWS::EC2::VPC` stays in the catch-all, so every VPC also has an `AwsConfigResource` twin.

**IAM:** the `connectivity` collector needs `ec2:DescribeVpcs`, `ec2:DescribeVpcPeeringConnections`, `ec2:DescribeTransitGateways` and `ec2:DescribeTransitGatewayAttachments`, all in `ReadOnlyAccess`. Without them the collector fails like any other, and retention applies.

## Resolution

`derive/networks.rs` holds `JOINS`, checked against `ALL_KINDS` like `CHAINS`. Each row names a network kind and how two networks of it are joined, over projected edges in either direction:

```
Through { network: AwsEc2Vpc, connector: AwsEc2VpcPeeringConnection }
Through { network: AwsEc2Vpc, connector: AwsEc2TransitGateway }
Direct  { network: AzureVirtualNetwork }
Direct  { network: GcpComputeNetwork }
```

`Through` joins the networks beside the same connector; `Direct` joins two networks with an edge between them.

`service::address_owners` uses them. A private pivot `a@N` that nothing in N holds takes the owners of `a@M` when **exactly one** network M joined to N holds `a`.

- **A local holder always wins**: an address held where it was read never looks across a join.
- **Joins don't chain.** A–B and B–C never joins A and C; peering is not transitive on any of the three clouds. A transit gateway is a hub that joins all its attachments to each other.
- **Ambiguity resolves to nothing**, the same rule as "owner edges only where the description is unambiguous". Peered networks can't overlap on any of the three clouds, so two holders mean something the model can't place.
- **The stopped-instance gate follows the join.** `link_targets` skips an IP target whose address would resolve, locally or across the one joined holder, to an interface attached to an instance the scan dropped; otherwise that interface would own itself and read as a service. It asks the resolver derive uses (`derive::held_addresses`), so the two can't disagree about where an address lands.
- **A connector is a hub, never expanded into pairs.** Two networks are joined when they share one, so a transit gateway with k attachments costs k entries, not k². Expanded, 4,000 attachments cost 4.7 s and 1.2 GB per derivation (release build), and derivation runs several times per reconcile plus once per snapshot; as a hub, 0.5 ms.
- **One place.** Because this sits in the ownership map, observed flows, IP-target chains (an NLB targeting an address in a peered VPC is `inferred`) and the service view all pick it up.

Pinned by `a_private_address_resolves_across_a_peering`, `a_private_address_resolves_across_a_transit_gateway`, `peering_does_not_chain`, `an_address_two_joined_networks_hold_resolves_to_neither`, `a_holder_in_the_reading_network_wins_over_any_joined_one`, `an_ip_target_in_a_peered_vpc_is_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred`, and the property test `an_unheld_address_resolves_through_exactly_one_joined_holder`.

**Known gaps.** Both are about transit gateway route tables, and phase 3 closes both.
- **Overlap.** A transit gateway may carry overlapping VPCs; its route tables decide which one wins. Without address spaces, phase 1 can't tell an address that is unheld but inside the reading VPC's own range from one that lives across the gateway, so it may resolve the first kind. This takes a misconfigured estate plus an unheld local address.
- **Segmentation.** Route tables can also isolate attachments that share a gateway, such as prod and dev each with its own table. Phase 1 joins every attachment to every other, so prod traffic to an address only an isolated dev VPC holds still resolves to the dev holder. The source interface's flow log records the attempt as accepted even though the gateway drops it, so a connection that never happened reads as `confirmed`.

## Fixture

Globex now carries all three joins:
- **AWS peering:** `vpc-globex-data` is peered with prod through `pcx-globex-data`; the warehouse `i-globex-warehouse` is at `10.40.1.20`.
- **AWS transit gateway:** `tgw-globex` joins prod and `vpc-globex-shared`, whose VPC-endpoint interface is at `10.50.0.10`.
- **Rejected peering, overlapping VPC:** `vpc-globex-staging` reuses prod's `10.10.0.0/16`, has its own `10.10.1.10`, and is joined to nothing. Its peering request was rejected for the overlap.
- **Route table:** prod's private route table routes to the peering and the gateway.
- **Azure:** `vnet-globex-hub` is peered with `vnet-globex`; a peering to `vnet-globex-dr` only one side has created joins nothing.
- **GCP:** `vpc-globex` is peered with the Cloud SQL producer network; an `INACTIVE` peering joins nothing.

Two flows cross joins: web-01 → `10.40.1.20:5439` and web-02 → `10.50.0.10:636`. They derive `web-01 -Serves-> i-globex-warehouse` and `web-02 -Serves-> AwsEc2Eni(eni-globex-ldap)`.

## Phases

| Phase | Deliverable | Status |
|---|---|---|
| 1 | Networks first-class with one identity each; AWS VPC peering and Transit Gateways (+ route targets); Azure VNet peering; GCP VPC peering; resolution across joins | Done |
| 2 | Interconnects: AWS VPN (VGW/CGW/VPN connections, TGW VPN attachments) and Direct Connect (new `aws-sdk-directconnect` dependency); Azure virtual network gateways, local network gateways, connections, ExpressRoute circuits; GCP HA VPN gateways, tunnels, external VPN gateways, Cloud Routers, interconnect attachments. Cross-cloud joins go through the public pivots both sides name: a tunnel's outside address on one side is the peer gateway's address on the other (`RoutesTo`, never `ConnectsTo`, since a gateway names its peer's address, it doesn't hold it). Azure gateway transit (`useRemoteGateways`). New `Join` forms: a tunnel joins the networks behind its two gateways, a path that `Through` and `Direct` can't express. Restores the fixture's AWS → Azure VM and AWS → Cloud SQL hops | Next |
| 3 | Address spaces (VPC CIDRs, VNet prefixes, GCP subnet ranges) without flooding `Covers`; route-table-precise resolution through VPC and TGW route tables; attributing an unheld address to the network whose range covers it; Tier-1 typed mappings for the new kinds; Virtual WAN and NCC hubs; TGW peering | Later |
