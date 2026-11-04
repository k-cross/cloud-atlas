# Network Connectivity: Networks, Address Spaces, Routing, Hubs, VPN & Dedicated Circuits

Private addresses are keyed per network (`GenericPrivateIpAddress { network, address }`, [`docs/network_inference_design.md`](network_inference_design.md) §1), keeping overlapping RFC 1918 ranges separate while requiring explicit network joins to resolve cross-network traffic. Node identity remains per network; cross-network resolution happens at derive time. Historical decisions live in [`docs/audit_findings.md`](audit_findings.md).

## 1. Network Identity

`AwsEc2Vpc`, `AzureVirtualNetwork`, and `GcpComputeNetwork` represent networks. Every network node's ID equals the `network` scope of its private addresses (`Node::network_scope()`):

| Provider | Network ID & Scope |
|---|---|
| **AWS** | VPC ID (`DescribeVpcs` places every VPC under its region, including empty and peer-only VPCs) |
| **Azure** | Lowercased VNet ARM ID (`canonical_azure_id`); NIC addresses derive their VNet from `subnet.id` |
| **GCP** | `projects/P/global/networks/N` via `canonical_network` (normalising full URLs, relative paths, and bare names like GKE's `network`; `networkConfig.network` takes precedence on GKE) |

**Address spaces.** A network `ConnectsTo` each range it owns, scoped to itself (`Node::ip(range, network)`). Every edge from a network kind to a range is address space, never a held host address:

| Provider | Address space |
|---|---|
| **AWS** | The VPC's primary `cidrBlock` and every `associated` IPv4 and IPv6 association (`DescribeVpcs`); a range being released is no longer drawn |
| **Azure** | The VNet's `addressSpace.addressPrefixes` |
| **GCP** | Each subnet's `ipCidrRange`, `secondaryIpRanges`, `internalIpv6Prefix` and `externalIpv6Prefix`, drawn from the network |

Containment (`derive::containment`) covers host addresses only from ranges a rule names (an incoming `RoutesTo`), so an address space adds one edge per range and no `Covers` edge per host. Ranges are not filtered by `is_large_cidr`: a `/8` VNet is still that VNet's space.

## 2. Modelled Joins

Joins project as standard graph edges using existing `Edge` kinds:

| Provider | Collected | Projected |
|---|---|---|
| **AWS** | `DescribeVpcPeeringConnections` | `AwsEc2VpcPeeringConnection -AttachedTo->` requester and accepter VPCs (`active` only) |
| **AWS** | `DescribeTransitGateways`, `DescribeTransitGatewayAttachments` | `Region -Contains-> AwsEc2TransitGateway` (`available`/`modifying`); `-AttachedTo->` each VPC with an `available`/`modifying` VPC attachment |
| **AWS** | Route tables | `Vpc -Contains-> RouteTable -Contains-> AwsEc2Route { table, destination }` for every CIDR or prefix-list route (the destination is the CIDR or the `pl-` id); `Route -RoutesTo->` its target: the VPC itself for `local`, or the NAT gateway, internet gateway, VPN gateway, peering connection, transit gateway or interface. A `blackhole` route, or one whose target is not modelled (an instance, endpoint or egress-only gateway), is drawn with no target, so it still wins longest-prefix match and goes nowhere |
| **AWS** | `DescribeTransitGatewayRouteTables`, then per `available` table `GetTransitGatewayRouteTableAssociations`, `GetTransitGatewayRouteTablePropagations` and `SearchTransitGatewayRoutes` (static, `active` or `blackhole`) | `Tgw -Contains-> TransitGatewayRouteTable`; `Resource -AttachedTo-> Table` per `associated`/`disassociating` association; `Table -RoutesTo-> Resource` per `enabled`/`disabling` propagation; `Table -Contains-> TransitGatewayRoute { table, destination } -RoutesTo->` each attachment of an `active` static route, whose destination is a CIDR or a prefix list. A resource is the attachment's VPC, VPN connection, Direct Connect gateway or peering attachment. Tables of a gateway the scan skips are not drawn |
| **AWS** | `DescribeTransitGatewayPeeringAttachments` | `Region -Contains-> TransitGatewayPeeringAttachment -AttachedTo->` both gateways (`available`/`modifying`); a peer gateway in another region or account is drawn bare |
| **Azure** | VNet `virtualNetworkPeerings` | `AzureVirtualNetwork -ConnectsTo-> AzureVirtualNetwork` (`Connected` only) |
| **Azure** | ARG `virtualwans` (with `type`), `virtualhubs`; ARM `virtualHubs/{hub}/hubVirtualNetworkConnections` per hub that is not `Deleting` or `Failed` | `VirtualHub -AttachedTo-> VirtualWan` on a `Standard` WAN; a `Basic` WAN `-Contains->` its hubs instead, as Basic hubs carry no traffic between VNets or between hubs; `VirtualHub -AttachedTo-> VNet` per connection. Any WAN, hub or connection that is `Deleting` or `Failed` is skipped |
| **GCP** | `networks.list` `peerings` | `GcpComputeNetwork -ConnectsTo-> GcpComputeNetwork` (`ACTIVE` only) |
| **GCP** | Network Connectivity Center `locations/global/hubs` and `locations/global/spokes` | `Project -Contains->` each hub and VPC spoke; `Spoke -AttachedTo-> Network`; `Spoke -AttachedTo-> Group` (its `group`, else the hub's `default`); `Hub -Contains-> Group`. Hubs and spokes are drawn while `ACTIVE` or `UPDATING`; a spoke on a hub the scan skips is not |

- **Filtering & lifecycle:** Blackhole routes (including NAT/IGW), rejected/deleted peerings, and pending attachments project nothing. `modifying` gateways and attachments stay projected so active paths do not flap during changes. Cross-account peer VPCs project as bare `AwsEc2Vpc` nodes.
- **AWS Config:** `AWS::EC2::VPC`, `RouteTable`, `VPCPeeringConnection`, `TransitGateway` and `TransitGatewayRouteTable` are in `TYPED_CONFIG_TYPES`, so the Config catch-all never draws them; `TransitGatewayAttachment`, drawn as an edge, is excluded in `use_aws_resource`. Config records no transit gateway peering. Tier-1 events for these kinds are in §7.
- **IAM:** Requires `ec2:DescribeVpcs`, `ec2:DescribeVpcPeeringConnections`, `ec2:DescribeTransitGateways`, `ec2:DescribeTransitGatewayAttachments`, `ec2:DescribeTransitGatewayRouteTables`, `ec2:GetTransitGatewayRouteTableAssociations`, `ec2:GetTransitGatewayRouteTablePropagations`, `ec2:SearchTransitGatewayRoutes` and `ec2:DescribeTransitGatewayPeeringAttachments`. The first four form one `connectivity` collector with the three VPN calls (§3); the route table and peering attachment calls form a separate `transit_gateway_routing` collector. A denied call fails its whole collector and the report names it. Losing `transit_gateway_routing` keeps VPCs, peerings, gateways and VPNs; once its retention runs out, transit gateway tables and peerings drop, so transit gateways route permissively and peered gateways stop joining. Which gateways the scan skips comes from every region's `connectivity`, so a table or peering attachment is never drawn against a gateway that is going away. Direct Connect has its own collector (§4). GCP needs `networkconnectivity.hubs.list` and `networkconnectivity.spokes.list`. The Network Connectivity API is off by default, and a GCP collector whose API is disabled in the scanned project reads as empty rather than failing; confirming that the `SERVICE_DISABLED` error names the scanned project (not the caller's quota project) needs `resourcemanager.projects.get`, and without it the collector fails as before. Azure needs `Microsoft.Network/virtualHubs/hubVirtualNetworkConnections/read`, and a hub whose connections cannot be read is reported as `hub_connections/<hub>`.
- **Hubs as paths:** A transit gateway, a Virtual WAN and an NCC hub are each one connector reached along a fixed path from its networks: `Vpc ← Tgw`, `VNet ← VirtualHub → VirtualWan` (VNets on any hub of one Standard WAN reach each other, as a Standard WAN routes between its hubs; a Basic WAN contains its hubs instead, so the path never completes), and `Network ← Spoke → Group ← Hub`. An NCC `STAR` hub puts spokes in a `center` or `edge` group; an edge spoke reaches the centre only.

## 3. Site-to-Site VPN

A tunnel is stitched through public addresses both ends already name (rule 5). Each end *holds* its own public address (`ConnectsTo`) and *names* the far end's as its peer (`RoutesTo`, since naming is not holding). A tunnel exists only when each end names an address the other holds, since IPsec needs both ends configured.

| Cloud | Collected | Projected |
|---|---|---|
| **AWS** | `DescribeVpnGateways`, `DescribeCustomerGateways`, `DescribeVpnConnections` (in the `connectivity` collector) | `Region -Contains->` each `available` gateway, customer gateway and connection; `Vgw -AttachedTo-> Vpc` (attached only); `VpnConnection -AttachedTo->` its `Vgw` or `TransitGateway` and its `CustomerGateway`; `VpnConnection -ConnectsTo->` each AWS tunnel outside address (`tunnel_options` and `vgw_telemetry`); `CustomerGateway -RoutesTo->` its peer address |
| **Azure** | ARG `virtualnetworkgateways`, `localnetworkgateways`, `connections` | `Vng -AttachedTo-> VNet` (from its subnet id); `Vng -HasIp-> AzurePublicIpAddress`; `Connection -AttachedTo->` its `Vng` and `Lng`; `Lng -RoutesTo->` its `gatewayIpAddress`; a Connected peering with `useRemoteGateways` also projects `spoke -RoutesTo-> hub`. VPN and ExpressRoute gateways are both `AzureVirtualNetworkGateway`, and every `connections` resource is one `AzureGatewayConnection` (§4). Any gateway, local gateway or connection whose `provisioningState` is `Deleting` or `Failed` is skipped |
| **GCP** | `aggregated/vpnGateways`, `aggregated/vpnTunnels`, `global/externalVpnGateways` | `VpnGateway -AttachedTo-> Network`; `VpnGateway -ConnectsTo->` each interface address; `Tunnel -AttachedTo->` its `VpnGateway` and `ExternalVpnGateway`; `ExternalVpnGateway -RoutesTo->` each interface address. Tunnels whose `status` is `PROVISIONING`, `ALLOCATING_RESOURCES`, `DEPROVISIONING` or `FAILED` are skipped |

Every cloud skips VPN resources that are not built yet or are being removed. Azure `Updating`, like AWS `modifying`, stays drawn. Tunnel health is runtime state (rule 6) and is not drawn: AWS `UP`/`DOWN`, Azure `connectionStatus`, and the GCP `status` values other than those four (`ESTABLISHED`, `NO_INCOMING_PACKETS`, `NEGOTIATION_FAILURE`, …). A tunnel configured at both ends joins whether or not it is up, and a resource with no state is drawn. Config types `VPNGateway`, `CustomerGateway` and `VPNConnection` are in `TYPED_CONFIG_TYPES` and have Tier-1 events (§7). **IAM adds** `ec2:DescribeVpnGateways`, `ec2:DescribeCustomerGateways` and `ec2:DescribeVpnConnections` (all in `ReadOnlyAccess`); Azure and GCP read them with the access they already need.

## 4. Dedicated Circuits

A dedicated circuit carries BGP sessions over a point-to-point link subnet (a `/29`–`/31`, usually link-local `169.254.0.0/16`). Each end holds its own BGP address and names its peer's, so a circuit pairs exactly like a tunnel. `Node::link_ip` builds each address as `GenericPrivateIpAddress { network: <link subnet>, address }`: a BGP address belongs to the link, not to either VPC, and both ends compute the same subnet from the address and its prefix. Two ends join their networks only when each names an address the other holds.

Only Cross-Cloud Interconnect puts two modelled clouds on one link: GCP to an AWS Direct Connect connection or an Azure ExpressRoute Direct port. Circuits that end at an on-premises or partner router (Dedicated and Partner Interconnect, hosted connections, provider ExpressRoute) are drawn but join nothing. The router between two such circuits is not in the scan, and each circuit has its own link subnet to it, so the circuits share no address.

| Cloud | Collected | Projected |
|---|---|---|
| **AWS** | `DescribeConnections` and `DescribeVirtualInterfaces` per region (`direct_connect`); `DescribeDirectConnectGateways` and `DescribeDirectConnectGatewayAssociations` per gateway once per scan, since both are account-wide (`direct_connect_gateways`, scope `global`, on the first region's client). All four page by hand | `Region -Contains->` each `available`/`down` connection and each private or transit VIF that is `available`, `down` or `testing`; `global -Contains->` each `available` Direct Connect gateway; `Vif -AttachedTo->` its connection (scanned connections only, so a LAG id draws nothing), its VGW and its Direct Connect gateway (the API reports an unset gateway as an empty id, which draws nothing); `Vgw`/`Tgw -AttachedTo-> DxGateway` for each `associated`/`updating` association; `Vif -ConnectsTo->` each Amazon BGP address and `-RoutesTo->` each customer address (from `available` `bgpPeers`, else the VIF's own pair). Public VIFs reach AWS public endpoints, not a VPC, and are skipped |
| **Azure** | ARG `expressroutecircuits`; ExpressRoute gateways and connections come through §3's types | `Connection -AttachedTo->` its circuit (`peer`); `Circuit -ConnectsTo->` the second usable address of each enabled private-peering prefix (IPv4 and IPv6) and `-RoutesTo->` the first, Azure's documented split. Microsoft peering is skipped, as is a circuit that is `Deleting`/`Failed` or whose provider state is `NotProvisioned`, `Provisioning` or `Deprovisioning` |
| **GCP** | `aggregated/routers`, `global/interconnects`, `aggregated/interconnectAttachments` | `Router -AttachedTo-> Network`; `Attachment -AttachedTo->` its router and interconnect; `Attachment -ConnectsTo->` its `cloudRouterIp(v6)Address` and `-RoutesTo->` its `customerRouterIp(v6)Address`. `UNPROVISIONED` interconnects are skipped, as are attachments that are `DEFUNCT`, `PENDING_PARTNER`, `PENDING_CUSTOMER`, `PARTNER_REQUEST_RECEIVED` or `UNPROVISIONED` |

A network reaches a circuit end through the gateway its traffic uses:
- **AWS:** a VIF on the VPC's VGW, on a Direct Connect gateway associated with that VGW, or on one associated with a transit gateway the VPC is attached to.
- **Azure:** a circuit through the hub's gateway connection, and through `useRemoteGateways` for a spoke.
- **GCP:** a VLAN attachment through the network's Cloud Router.

A reference never redraws a resource the scan lists but skips: an Azure connection draws no edge to a skipped gateway, local gateway or circuit, a VIF or association none to a Direct Connect gateway that is not `available`, and an attachment none to an `UNPROVISIONED` interconnect. A reference to a resource the scan does not list (another account or subscription) still draws it bare. A Direct Connect gateway does not route between the gateways on it, so VPCs behind one Direct Connect gateway are not joined to each other. BGP and port health (`bgpStatus`, VIF `down`, `operationalStatus`) is runtime state and is not drawn. AWS Config records no Direct Connect type, so the three kinds have no Tier-1 path and are listed as kinds Config does not record. **IAM adds** `directconnect:DescribeConnections`, `directconnect:DescribeVirtualInterfaces`, `directconnect:DescribeDirectConnectGateways` and `directconnect:DescribeDirectConnectGatewayAssociations` (all in `ReadOnlyAccess`). Direct Connect is a separate service with its own client, so it is a separate collector, and a Direct Connect failure never costs the VPCs and peerings that `connectivity` reads.

## 5. Cross-Network Address Resolution

`derive/networks.rs` defines `JOINS` (validated against `ALL_KINDS`). Each step names its edge's direction (`to` an outgoing edge, `from` an incoming one); `Direct` matches any projected edge between two networks:

```
Hub        { network: AwsEc2Vpc, path: [from AttachedTo AwsEc2VpcPeeringConnection] }
Hub        { network: AwsEc2Vpc, path: [from AttachedTo AwsEc2TransitGateway] }
HubPeering { hub: AwsEc2TransitGateway, peering: AwsEc2TransitGatewayPeeringAttachment }
Hub        { network: AzureVirtualNetwork, path: [from AttachedTo AzureVirtualHub, to AttachedTo AzureVirtualWan] }
Hub        { network: GcpComputeNetwork, path: [from AttachedTo Spoke, to AttachedTo Group, from Contains Hub], edge: /groups/edge }
Direct     { network: AzureVirtualNetwork }
Direct     { network: GcpComputeNetwork }
Link       { network, ends: [[step ..] ..], holds: [[step .. address] ..], names: [[step .. address] ..] }
```

A hub row walks its path backwards from every hub node to the networks behind it, so a hub costs its own attachments. Every membership records the **approach**: the node after the network on the path (the peering, transit gateway, VPN gateway, virtual hub or spoke the traffic leaves through) and the node after that. Transit gateway peering joins a member of one gateway to a member of the gateway it peers with, one hop only.

Each cloud has one `Link` row for VPN and one for circuits. A row finds its link ends by walking each `ends` path backwards from every node of the end kind to the networks behind it, so a hub with no VPN connection or VIF on it costs nothing, and each end then walks `holds` and `names` to addresses: public tunnel addresses, or link-scoped BGP addresses. No two rows share an end kind (`every_link_end_kind_belongs_to_one_row`), because an end keeps the `holds` and `names` of the row that reaches it first. The ends are:
- **VPN:** the VPN connection on AWS, and the gateway on Azure and GCP.
- **Circuits:** the VIF on AWS, the circuit on Azure, and the VLAN attachment on GCP.

**Two ends form a link only when each names an address the other holds, and only then are the networks behind them joined.** One-sided naming joins nothing, so a far end that is deleted, failed or never configured breaks the join. Ends pair one by one: a VGW with one connection to Azure and another to an office cannot combine halves of the two into a tunnel. Each link is a connector with two sides, so two VPCs tunnelled to the same Azure gateway, or two VPCs behind one Direct Connect gateway, are not joined to each other. Controlled transitivity is just a longer `ends` path: a transit gateway's VPN connection is an end for every VPC attached to it, and an Azure spoke reaches the hub's gateway through the `RoutesTo` edge that only `useRemoteGateways` projects, so a plain peering never rides the gateway, and the hub never rides a spoke's gateway back along that edge. Private addresses never act as connectors.

In `service::address_owners`, an unheld private pivot `a@N` inherits the owners of `a@M` when **exactly one network `M` joined to `N` holds `a` and routing lets traffic from `N` reach it** (`HeldAddresses::holder`):

- **Local holder wins:** An address held in `N` never looks across a join.
- **Local space wins:** An unheld address inside `N`'s own address space belongs to `N`, so it resolves to nothing rather than to an overlapping network across a transit gateway.
- **VPC routes decide the way out:** When `N` has route tables in the graph, some table's longest-prefix match for `a` must target the approach's gateway (the peering, transit gateway or VPN gateway). Tables are not tied to the subnet the traffic came from, so any table in `N` that routes `a` that way is enough. A blackhole or unmodelled target at the longest match blocks the crossing. A prefix-list route's entries are not read, so it may be the longest match for any address: one that targets the gateway allows the crossing whatever the CIDR routes say. A VPC whose route tables were not read crosses every join.
- **Transit gateway tables decide the way through:** When the gateway has route tables in the graph, the table `N`'s attachment is associated with must route `a` to the far attachment: the far VPC, the VPN connection or Direct Connect gateway on the approach, or the peering attachment. Routes are its static routes (a prefix-list route counts as above) plus each propagation; a VPC's propagation carries its address space, and a VPN or Direct Connect propagation carries routes learned over BGP that the scan cannot see, so it matches any address at the lowest precedence. Longest prefix wins, then static over propagated. An attachment associated with no table reaches nothing. Across a peering, the far gateway's table associated with the peering attachment must route `a` on to `M`.
- **Non-transitive:** `A–B` and `B–C` never join `A` to `C`. Transit gateways act as hubs joining all attached VPCs.
- **Ambiguity resolves to nothing:** If two joined networks hold `a`, neither wins.
- **Stopped-instance gate:** `link_targets` queries `derive::held_addresses` so IP targets resolving locally or across a join to an unscanned instance's ENI are skipped.
- **Hub representation:** Connectors stay hubs ($O(k)$ attachments, never expanded into $k^2$ pairs).
- **Single integration point:** Sitting inside `address_owners`, resolution automatically feeds observed flows, IP-target chains (`NLB` targeting a peered VPC IP → `inferred`), and the service view.

Pinned by `an_unheld_address_inside_the_reading_network_stays_local`, `a_route_table_chooses_which_join_an_address_crosses`, `a_network_with_no_route_toward_a_join_does_not_cross_it`, `a_more_specific_blackhole_route_stops_the_crossing`, `a_route_to_a_prefix_list_may_carry_any_address`, `a_transit_gateway_route_table_segments_its_attachments`, `a_vpn_on_a_transit_gateway_needs_its_table_to_carry_the_far_side`, `transit_gateway_peering_needs_a_route_on_both_gateways_and_goes_one_hop`, `a_mesh_hub_joins_every_spoke_and_a_star_hub_keeps_edges_apart`, `virtual_networks_on_one_virtual_wan_are_joined_across_its_hubs`, `only_a_standard_virtual_wan_carries_traffic_between_its_networks`, `a_private_address_resolves_across_a_peering`, `a_private_address_resolves_across_a_transit_gateway`, `peering_does_not_chain`, `an_address_two_joined_networks_hold_resolves_to_neither`, `a_holder_in_the_reading_network_wins_over_any_joined_one`, `an_ip_target_in_a_peered_vpc_is_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred`, `a_tunnel_joins_the_networks_at_its_two_ends`, `a_tunnel_configured_at_one_end_joins_nothing`, `a_gateway_pairs_each_of_its_connections_on_its_own`, `a_tunnel_whose_far_end_is_going_away_resolves_nothing`, `a_direct_connect_interface_and_an_interconnect_attachment_join_their_networks`, `networks_behind_one_direct_connect_gateway_reach_its_circuit_but_not_each_other`, `an_expressroute_circuit_joins_the_hub_and_the_spokes_that_use_its_gateway`, `circuits_that_end_at_routers_the_scan_does_not_model_join_nothing`, `a_private_address_resolves_across_a_dedicated_circuit`, `a_circuit_whose_far_end_is_going_away_resolves_nothing`, and property tests `an_unheld_address_resolves_through_exactly_one_joined_holder`, `a_tunnel_joins_two_networks_only_when_each_end_names_the_other` and `both_ends_of_a_link_read_their_addresses_in_the_subnet_they_share`.

### Known Gaps

- **Not modelled (Phase 2a):** classic GCP VPN (`targetVpnGateways`), HA VPN between two GCP gateways, Azure VNet-to-VNet connections between two modelled gateways, GCP `exportCustomRoutes` transit through peerings (Cloud SQL private services access is read in the consumer network, so it is unaffected), and BGP-learned routes.
- **Not modelled (Phase 2b):** on-premises and partner routers (Megaport, Equinix Fabric), so circuits that meet there join nothing; Direct Connect LAGs (a VIF on a LAG is drawn without its connection); SiteLink; ExpressRoute Global Reach, ExpressRoute Direct ports and Virtual WAN ExpressRoute gateways; Direct Connect gateway `allowedPrefixes`; GCP attachment `adminEnabled`.
- **Link-address collisions:** A link address is unique only within its link. Two unrelated circuits whose subnets coincide, with each holding the address the other names, would pair. Auto-assigned link-local subnets are drawn from `169.254.0.0/16`, so this takes a roughly 1-in-16,000 coincidence within one estate.
- **Link-to-link transit:** Two VPN connections or VIFs on one transit gateway route to each other, as do two VPNs on one VGW (CloudHub), two ExpressRoute connections on one gateway, and VNets whose gateways connect to one ExpressRoute circuit. A link join only reaches networks, and networks on one side of a link are not joined to each other, so these networks are not joined.
- **Routing outside AWS:** Azure user-defined routes and GCP routes (custom static, policy-based and peering custom routes) are not read, so Azure and GCP joins are not route-checked. Virtual WAN custom route tables and routing intent, NCC `CUSTOM` policy mode, hybrid and producer-VPC spokes and NCC export filters are not modelled; VNets on one WAN and spokes on one hub join as if default routing applied.
- **Route-table approximations:** A VPC route is checked against every table in the VPC, not the table of the source subnet. Middlebox routing (a route to an interface or Gateway Load Balancer endpoint that then forwards over a join) blocks the crossing. Transit gateway Connect, VPN concentrator, network function and Cloud WAN attachments are not targets. Prefix lists' entries are not read, so a prefix-list route may carry any address on both route types.
- **Learned prefixes:** BGP routes learned over VPN and Direct Connect are not read, so a transit gateway table that propagates such an attachment routes any address there that no known prefix claims.

## 6. Globex Fixture Coverage

- **Address spaces & routes:** every VPC carries its CIDR; the VNets `10.20.1.0/24` (spoke), `10.20.100.0/24` (hub), `10.22.0.0/16` and `10.23.0.0/16`; and GCP's `subnet-data` `10.30.0.0/24`. Prod's private table routes `10.40.0.0/16` to the peering, `10.50.0.0/16`, `10.20.0.0/24` and `10.70.0.0/16` to the transit gateway, and `10.20.1.0/24` to `vgw-globex`; the data VPC's `rtb-data` routes back over the peering, sends `10.20.0.0/24` to `vgw-globex-data`, and keeps a blackhole route to a retired peering.
- **Transit gateway routing & peering:** `tgw-rtb-globex` associates and propagates prod, shared and `vpn-globex-gcp`, associates the peering attachment, sends `10.70.0.0/16` to it and blackholes `10.66.0.0/16`. `tgw-globex` peers with `tgw-globex-west` in `us-west-2`, whose table routes `10.10.0.0/16` back and propagates `vpc-globex-west`, so `web-02 -Serves-> AwsEc2Eni(eni-globex-west-api)` (`10.70.1.15`) resolves across both gateways' tables.
- **Hubs:** the NCC hub `globex-hub` meshes `vpc-globex` with `vpc-globex-analytics`; the Virtual WAN `vwan-globex` connects `vnet-globex-apps` and `vnet-globex-data` through `vhub-globex-eastus`.

- **AWS peering:** `vpc-globex` ↔ `vpc-globex-data` via `pcx-globex-data` (`i-globex-warehouse` at `10.40.1.20`; flow from `web-01` derives `web-01 -Serves-> i-globex-warehouse`).
- **AWS TGW:** `tgw-globex` joins `vpc-globex` and `vpc-globex-shared` (`eni-globex-ldap` at `10.50.0.10`; flow from `web-02` derives `web-02 -Serves-> AwsEc2Eni(eni-globex-ldap)`). Prod's private route table routes to both `pcx-globex-data` and `tgw-globex`.
- **Rejected peering & overlap:** `vpc-globex-staging` reuses `10.10.0.0/16` (`10.10.1.10`) with a rejected peering request—isolated from prod.
- **VPN to Azure:** prod's `vgw-globex` and `cgw-globex-azure` (the hub gateway's public IP) reach `vng-globex-hub`, whose `lng-globex-aws` names an AWS tunnel address. `vnet-globex` peers with the hub using `useRemoteGateways`, so `web-01 -Serves-> vm-worker` (`10.20.1.4`) resolves through the hub.
- **VPN to GCP:** `tgw-globex` carries `vpn-globex-gcp` to the HA VPN gateway on `vpc-globex` (`ha-gw-globex`, `tun-aws-0`, external gateway `aws-peer`), so `web-02 -Serves-> globex-analytics-db` (`10.20.0.5`) resolves.
- **Direct Connect to GCP:** `vpc-globex-data`'s `vgw-globex-data` is associated with `dxgw-globex`. Its private VIF `dxvif-globex-gcp` on `dxcon-globex-gcp` peers on `169.254.60.0/30` with the Cross-Cloud Interconnect attachment `att-globex-aws` (router `rtr-globex-interconnect` on `vpc-globex`), so `i-globex-warehouse -Serves-> globex-analytics-db` resolves.
- **ExpressRoute to GCP:** the hub's ExpressRoute gateway `ergw-globex-hub` connects to `er-globex-gcp`, whose primary private-peering link `169.254.70.0/30` pairs with `att-globex-azure`. The spoke rides it by gateway transit, so `10.20.0.5` read in the spoke resolves to Cloud SQL; no Azure flow log feeds the demo yet, so no `Serves` edge shows it. The secondary link names nothing modelled. The two circuits meet in GCP but do not join the data VPC to Azure.
- **Azure & GCP:** `vnet-globex-hub` ↔ `vnet-globex` (`Connected`), plus a one-sided `vnet-globex-dr` peering (ignored); `vpc-globex` ↔ Cloud SQL producer network (`ACTIVE`), plus an `INACTIVE` peering (ignored).

## 7. Tier-1 Events

AWS Config items for the connectivity kinds become typed events through the same projector helpers the scan uses (`project_vpc`, `project_route_table`, `project_peering`, `project_vpn_gateway`, `project_customer_gateway`, `project_vpn_connection` with their `*Facts`), so an event draws only what a scan would (`every_connectivity_event_produces_only_what_a_full_scan_would`):

- **VPC** carries its address space, and a **route table** its routes and their targets (`a_route_table_event_carries_its_routes`).
- **Peerings, VPN gateways, customer gateways and VPN connections** read their state from the item's configuration (a string, or an object with `code` or `value`). A state the scan draws (`active`, `available`) creates or modifies; any other state removes the node, as the next scan would (`a_connectivity_item_in_a_state_the_scan_skips_removes_the_node`).
- **Transit gateways and their route tables** record no state in Config, so a creation or change waits for the scan and only a deletion is applied (`a_transit_gateway_item_can_only_remove_what_the_scan_drew`).
- Transit gateway attachments, associations, propagations and routes are edges or route nodes inside a scan, so they change with the next scan. Config records no Direct Connect type and no transit gateway peering.

## 8. Phases

| Phase | Deliverable | Status |
|---|---|---|
| **2a** | **Site-to-site VPN** on all three clouds with sided tunnel joins and Azure gateway transit (above). | Done |
| **2b** | **Dedicated circuits:** Direct Connect, ExpressRoute and Cloud Interconnect, paired on link-scoped BGP addresses; Cross-Cloud Interconnect is the only cross-cloud pairing (above). | Done |
| **3** | **Address spaces & routing precision:** VPC CIDRs, VNet prefixes and GCP subnet ranges as address space that covers nothing; unheld addresses inside the reading network's own space stay local; VPC and transit gateway route tables enforced during resolution; transit gateway peering; Virtual WAN and NCC hubs; Tier-1 events for the AWS connectivity kinds (above). | Done |
| **4** | **Routing beyond AWS & transit between links:** Azure user-defined routes and GCP routes; Virtual WAN route tables and routing intent; NCC hybrid spokes and export filters; link-to-link transit (VPN and circuits on one hub routing to each other); BGP-learned prefixes where an API reports them. | Next |
