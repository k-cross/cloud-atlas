# Network Connectivity: Networks, Peering, Transit Gateways & VPN

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
- **AWS Config:** `AWS::EC2::VPC`, `VPCPeeringConnection`, and `TransitGateway` are in `TYPED_CONFIG_TYPES`, so the Config catch-all never draws them; `TransitGatewayAttachment`, drawn as an edge, is excluded in `use_aws_resource`. Peerings and gateways have no event key: the scan filters them by state a Config item does not carry, so events leave them to the scan.
- **IAM:** Requires `ec2:DescribeVpcs`, `ec2:DescribeVpcPeeringConnections`, `ec2:DescribeTransitGateways`, and `ec2:DescribeTransitGatewayAttachments` (all included in `ReadOnlyAccess`). They form one `connectivity` collector with the three VPN calls (§3), so a denied call fails all seven and the report names `connectivity`.

## 3. Site-to-Site VPN

A tunnel is stitched through public addresses both ends already name (rule 5). Each side's gateway *holds* its own public address (`ConnectsTo`); the far side *names* it (`RoutesTo`, since naming is not holding). The two views meet on one `GenericIpAddress`, which is the connector.

| Cloud | Collected | Projected |
|---|---|---|
| **AWS** | `DescribeVpnGateways`, `DescribeCustomerGateways`, `DescribeVpnConnections` (in the `connectivity` collector) | `Region -Contains->` each `available` gateway, customer gateway and connection; `Vgw -AttachedTo-> Vpc` (attached only); `VpnConnection -AttachedTo->` its `Vgw` or `TransitGateway` and its `CustomerGateway`; `VpnConnection -ConnectsTo->` each AWS tunnel outside address (`tunnel_options` and `vgw_telemetry`); `CustomerGateway -RoutesTo->` its peer address |
| **Azure** | ARG `virtualnetworkgateways`, `localnetworkgateways`, `connections` | `Vng -AttachedTo-> VNet` (from its subnet id); `Vng -HasIp-> AzurePublicIpAddress`; `Connection -AttachedTo->` its `Vng` and `Lng`; `Lng -RoutesTo->` its `gatewayIpAddress`; a Connected peering with `useRemoteGateways` also projects `spoke -RoutesTo-> hub`. ExpressRoute gateways and connections are skipped, as is any gateway, local gateway or connection whose `provisioningState` is `Deleting` or `Failed` |
| **GCP** | `aggregated/vpnGateways`, `aggregated/vpnTunnels`, `global/externalVpnGateways` | `VpnGateway -AttachedTo-> Network`; `VpnGateway -ConnectsTo->` each interface address; `Tunnel -AttachedTo->` its `VpnGateway` and `ExternalVpnGateway`; `ExternalVpnGateway -RoutesTo->` each interface address. Tunnels whose `status` is `PROVISIONING`, `ALLOCATING_RESOURCES`, `DEPROVISIONING` or `FAILED` are skipped |

Every cloud skips VPN resources that are not built yet or are being removed. Azure `Updating`, like AWS `modifying`, stays drawn. Tunnel health is runtime state (rule 6) and is not drawn: AWS `UP`/`DOWN`, Azure `connectionStatus`, and the GCP `status` values other than those four (`ESTABLISHED`, `NO_INCOMING_PACKETS`, `NEGOTIATION_FAILURE`, …). A configured tunnel joins whether or not it is up, and a resource with no state is drawn. Config types `VPNGateway`, `CustomerGateway` and `VPNConnection` are in `TYPED_CONFIG_TYPES` with no event key, so events leave them to the scan. **IAM adds** `ec2:DescribeVpnGateways`, `ec2:DescribeCustomerGateways` and `ec2:DescribeVpnConnections` (all in `ReadOnlyAccess`); Azure and GCP read them with the access they already need.

## 4. Cross-Network Address Resolution

`derive/networks.rs` defines `JOINS` (validated against `ALL_KINDS`), matching projected edges in either direction:

```
Through { network: AwsEc2Vpc, connector: AwsEc2VpcPeeringConnection }
Through { network: AwsEc2Vpc, connector: AwsEc2TransitGateway }
Direct  { network: AzureVirtualNetwork }
Direct  { network: GcpComputeNetwork }
Tunnel  { network, gateways: [[(Edge, kind) ..] ..], holds: [(Edge, kind) .. GenericIpAddress], names: [(Edge, kind) .. GenericIpAddress] }
```

A `Tunnel` row reaches a network's gateways over any of its `gateways` paths, then walks `holds` or `names` from each gateway to a public address. A network that reaches address `X` over the *holding* suffix is a holder of `X`; over the *naming* suffix, a namer. **Two networks are joined through `X` only when one holds it and the other names it**, so two networks that both name one gateway are not joined to each other. Controlled transitivity is just a longer path: a transit gateway with a VPN attachment reaches `X` through every VPC attached to it, and an Azure spoke reaches the hub's gateway through the `RoutesTo` edge that only `useRemoteGateways` projects, so a plain peering never rides the gateway. Private addresses never act as connectors.

In `service::address_owners`, an unheld private pivot `a@N` inherits the owners of `a@M` when **at most one—and exactly one—network `M` joined to `N` holds `a`**:

- **Local holder wins:** An address held in `N` never looks across a join.
- **Non-transitive:** `A–B` and `B–C` never join `A` to `C`. Transit gateways act as hubs joining all attached VPCs.
- **Ambiguity resolves to nothing:** If two joined networks hold `a`, neither wins.
- **Stopped-instance gate:** `link_targets` queries `derive::held_addresses` so IP targets resolving locally or across a join to an unscanned instance's ENI are skipped.
- **Hub representation:** Connectors stay hubs ($O(k)$ attachments, never expanded into $k^2$ pairs).
- **Single integration point:** Sitting inside `address_owners`, resolution automatically feeds observed flows, IP-target chains (`NLB` targeting a peered VPC IP → `inferred`), and the service view.

Pinned by `a_private_address_resolves_across_a_peering`, `a_private_address_resolves_across_a_transit_gateway`, `peering_does_not_chain`, `an_address_two_joined_networks_hold_resolves_to_neither`, `a_holder_in_the_reading_network_wins_over_any_joined_one`, `an_ip_target_in_a_peered_vpc_is_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred`, and property test `an_unheld_address_resolves_through_exactly_one_joined_holder`.

### Known Gaps

- **Not modelled (Phase 2a):** classic GCP VPN (`targetVpnGateways`), HA VPN between two GCP gateways, Azure VNet-to-VNet connections between two modelled gateways, GCP `exportCustomRoutes` transit through peerings (Cloud SQL private services access is read in the consumer network, so it is unaffected), and BGP-learned routes.
- **One-ended tunnels:** A join needs only one end to name an address the other end holds. An Azure or GCP gateway holds its address without any connection, so an AWS connection naming it joins the two networks even when the Azure connection or GCP tunnel is missing, failed or being deleted. Requiring both ends (each names an address the other holds) would make every lifecycle filter break the join.
- **VPN-to-VPN transit:** Two VPN connections on one transit gateway (or on one VGW, as CloudHub) route to each other, but a tunnel join only reaches VPCs, so their far networks are not joined.
- **TGW overlap (Phase 3):** Without VPC CIDR ranges, an unheld address inside the reading VPC's own CIDR can resolve across a TGW to an overlapping attached VPC.
- **TGW route-table segmentation (Phase 3):** All VPC attachments on a TGW currently join each other regardless of per-attachment TGW route tables, so traffic attempted toward an isolated attachment's IP (logged as accepted on the source ENI before the TGW drops it) resolves to that holder.

## 5. Globex Fixture Coverage

- **AWS peering:** `vpc-globex` ↔ `vpc-globex-data` via `pcx-globex-data` (`i-globex-warehouse` at `10.40.1.20`; flow from `web-01` derives `web-01 -Serves-> i-globex-warehouse`).
- **AWS TGW:** `tgw-globex` joins `vpc-globex` and `vpc-globex-shared` (`eni-globex-ldap` at `10.50.0.10`; flow from `web-02` derives `web-02 -Serves-> AwsEc2Eni(eni-globex-ldap)`). Prod's private route table routes to both `pcx-globex-data` and `tgw-globex`.
- **Rejected peering & overlap:** `vpc-globex-staging` reuses `10.10.0.0/16` (`10.10.1.10`) with a rejected peering request—isolated from prod.
- **VPN to Azure:** prod's `vgw-globex` and `cgw-globex-azure` (the hub gateway's public IP) reach `vng-globex-hub`, whose `lng-globex-aws` names an AWS tunnel address. `vnet-globex` peers with the hub using `useRemoteGateways`, so `web-01 -Serves-> vm-worker` (`10.20.1.4`) resolves through the hub.
- **VPN to GCP:** `tgw-globex` carries `vpn-globex-gcp` to the HA VPN gateway on `vpc-globex` (`ha-gw-globex`, `tun-aws-0`, external gateway `aws-peer`), so `web-02 -Serves-> globex-analytics-db` (`10.20.0.5`) resolves.
- **Azure & GCP:** `vnet-globex-hub` ↔ `vnet-globex` (`Connected`), plus a one-sided `vnet-globex-dr` peering (ignored); `vpc-globex` ↔ Cloud SQL producer network (`ACTIVE`), plus an `INACTIVE` peering (ignored).

## 6. Phases

| Phase | Deliverable | Status |
|---|---|---|
| **2a** | **Site-to-site VPN** on all three clouds with sided tunnel joins and Azure gateway transit (above). | Done |
| **2b** | **Dedicated circuits:** Direct Connect (`aws-sdk-directconnect`), ExpressRoute circuits and gateways, Cloud Interconnect attachments and Cloud Routers. The clouds meet at a colocation partner with no shared public identifier, so cross-cloud matching needs its own design (BGP peer addresses, circuit keys). | Next |
| **3** | **Address spaces & routing precision:** Project VPC CIDRs, VNet prefixes, and GCP subnet ranges without flooding `Covers`; attribute unheld local addresses to covering networks; enforce VPC and TGW route tables during resolution; add Tier-1 mappings for connectivity kinds, Virtual WAN / NCC hubs, and TGW peering. | Later |
