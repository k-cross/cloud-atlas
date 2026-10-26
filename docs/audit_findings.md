# Audit Findings & Decision Record

Consolidated record of resolved bugs, rejected alternatives, and historical design decisions. Design documents describe only the current state; historical rationale lives here so invariants are not accidentally undone.

## Tier 1 (Event Feed)

### SQS as the event consumer
EventBridge rules route AWS Config items, EC2 state-change notifications, and CloudTrail management events to SQS. A queue buffers across server restarts, and its at-least-once delivery is handled by `EventApplier`. Config items carry attachments needed to project a creation; EC2 state changes are immediate and free but identity-only; CloudTrail provides the broadest API coverage.

### An interface with no reported subnet attaches to nothing
When `configuration.networkInterfaces` is absent, the adapter reads interface IDs from relationships, which carry no subnet. `project_instance` previously defaulted to the instance's subnet—wrong for secondary ENIs, causing events and full scans to disagree indefinitely. `EniFacts { subnet_id: None }` now emits no `AttachedTo` edge; the next scan supplies it. A missing edge heals; a wrong one flaps.
- Tests: `an_eni_from_relationships_alone_attaches_to_no_subnet`, `an_instance_event_produces_only_what_a_full_scan_would`.

### Config ENIs require full-scan ENI coverage and omit deferred owner edges
`AWS::EC2::NetworkInterface` Config items could not be mapped until `DescribeNetworkInterfaces` projected every ENI in full scans. Event adapters still omit both `link_interface_owners` edges (load balancer and instance attachment), as each requires cross-collection state absent from a single event.
- Tests: `an_interface_event_produces_only_what_a_full_scan_would`, `a_balancer_interface_event_does_not_invent_the_balancer`.

### Prune ordering on a tie without emptying the table
`EventApplier::prune` previously dropped entries at the cutoff timestamp (`<= cutoff`), which could empty the ordering table on a batch sharing one timestamp and let a redelivered create resurrect a deleted resource. `prune` shares `eviction_cut`/`survives` with `FlowIndex`.
- Test: `pruning_keeps_a_batch_that_shares_one_timestamp`.

## Tier 2 (Flow Logs)

### S3 → SQS over CloudWatch → Kinesis
VPC Flow Logs deliver to S3 with native SQS object-created notifications—zero intermediate compute. The CloudWatch Logs → Kinesis path required a per-shard consumer and a Lambda forwarder for no additional signal.

### Delete a notification only after its objects are read
`FlowLogQueue::receive` previously deleted SQS messages even when S3 reads failed transiently, losing flow objects permanently. Unlike Tier 1 (local parse only), Tier 2 performs a network fetch between receive and delete. `read_object` returns `Option`: `None` leaves the message queued for retry (including auth failures); `Some` deletes it (including `Malformed` content that would never parse).
- **404/410 exception:** Missing S3 objects are permanent. Retrying a multi-record notification would re-observe sibling objects indefinitely and inflate counters, so 404/410 is recorded as `Malformed` and the message is deleted.
- **Trade-off:** A partially failed notification is redelivered whole, double-counting sibling objects until their entries expire—preferable to dropping data.
- Tests: `an_object_that_could_not_be_read_leaves_its_notification_on_the_queue`, `an_object_that_was_read_but_makes_no_sense_is_deleted_anyway`, `an_object_that_is_gone_releases_its_notification`.

### Bound the merge context by what the index kept
`ingest_flows` previously built its merge subgraph from all arriving records (`MAX_RECORDS` = 100,000) even after `FlowIndex` evicted excess entries (`DEFAULT_CAPACITY` = 10,000), writing up to 10× the budget into the live graph. `FlowIndex::context` filters by retained entries, and `DEFAULT_CAPACITY` prevents busy VPCs talking to arbitrary external IPs from pinning pivot nodes for the life of the process.
- Test: `the_merge_context_never_exceeds_what_the_index_kept`.

### Evict on a tie without emptying the index
`evict` previously kept `last_seen > cutoff`. Because flow records have one-second precision, a single-second batch sharing the cutoff timestamp wiped the index. `eviction_cut` returns the cutoff plus a tie budget.
- Test: `eviction_survives_a_batch_that_shares_one_timestamp`.

### Truncate on a character boundary
`read_to_string` followed by `String::truncate(limit)` panicked or failed when the byte limit split a multi-byte UTF-8 character. `decompress` slices raw bytes at a valid character boundary before decoding.
- Test: `a_cut_that_lands_inside_a_character_still_yields_the_part_before_it`.

## Tier 3 (Reconciliation & Patching)

### Scan vs. event race is self-healing
Reconciliation installs the estate as of scan *start*. A resource created by a Tier-1 event mid-scan is removed when the scan lands and re-added on the next tick—briefly behind, never permanently inconsistent.

### Snapshot v3 separates observations from topology
Liveness updates far more frequently than topology. Snapshot v3 moves counters, status, and `last_seen` into sideband `observations` and `expired` lists so patches update freshness without re-announcing nodes or edges.

### Carry an edge by its owner, not by the pivot it touches
`carry_forward` previously retained an edge if *either* endpoint was held, and ownerless pivots count as held whenever referenced. A healthy provider's deleted edge to a shared pivot therefore reappeared every tick while any other provider was unreadable. Edges are now carried only when every owned endpoint is held.
- Test: `carry_forward_lets_a_healthy_source_delete_its_edge_to_a_shared_pivot`.

### No projected edge may join two ownerless pivots
An edge with no owned endpoints passes the owner check vacuously and anchors its own pivots. Cloudflare previously projected `hostname -ResolvesTo-> target`, preventing healthy zones from repointing or deleting records during another provider's outage. Both Cloudflare and Route 53 now project `hostname -ResolvesTo-> record -ResolvesTo-> target` (also fixing Route 53's record-to-name edge from `RoutesTo` to `ResolvesTo`).
- Tests: `every_projected_edge_has_an_owned_endpoint`, `carry_forward_lets_a_healthy_dns_provider_repoint_and_delete_records`.

### Page every AWS listing
Five AWS collectors read only their first page—Config (100 per type), Lambda (50), Route 53 zones (100), ELBv2 balancers, groups and listeners, and EventBridge buses—so a large estate's tail read as deleted. EC2 instances made one unpaged call as well. Eight more hand-rolled loops stopped only on a missing token and would spin forever on a repeated one, and Route 53 record sets resumed without `NextRecordIdentifier`, restarting at the first weighted or latency record of a name and type. ECS called `DescribeClusters` with no names, which describes only the `default` cluster. Every listing now uses the SDK's `into_paginator().items()`; Route 53 record sets and EventBridge buses (no SDK paginator) loop by hand and fail on a cursor that does not advance. ECS lists its clusters and describes them in batches of 100, treating `MISSING` as gone and any other failure as an error.
- Tests: `lambda_functions_follow_every_page`, `ecs_describes_every_listed_cluster_not_just_the_default`, `an_ecs_cluster_that_could_not_be_described_is_an_error_not_an_absence`, `eventbridge_buses_follow_every_page`, `eventbridge_refuses_a_page_token_that_repeats`, `route53_resumes_weighted_records_at_their_set_identifier`, `route53_refuses_a_record_cursor_that_does_not_advance`.

### One source per AWS resource
The Config catch-all drew an `AwsConfigResource` twin beside many typed nodes (VPCs, instances, subnets, security groups, queues, clusters, balancers), and a Config event emitted both. `TYPED_CONFIG_TYPES` now lists every Config type a typed node models and drives both the catch-all exclusion and the event mapping; the Config collector no longer lists types the projector would never draw. Config remains the source only for types no collector models (S3 buckets, RDS clusters, classic ELBs). Consequences: stopped instances, which only Config listed, and IAM roles nothing references no longer appear; ELBv2 listeners and transit gateway attachments are edges, not nodes.
- Tests: `every_aws_kind_config_records_has_one_source`, `a_config_listing_never_redraws_a_resource_its_own_collector_draws`, `a_config_item_for_a_typed_resource_yields_only_the_typed_node`.

## Networking & Pivots

### A private address is unique only inside one network
`GenericIpAddress` was originally keyed by address string alone, merging `10.0.1.5` or default-VPC `172.31.0.0/16` addresses across every VPC, account, region, and cloud. Private ranges now project as `GenericPrivateIpAddress { network, address }` via `Node::ip(value, network)`. Flow records resolve private IPs in the capturing interface's network and are dropped (`Malformed`) when the network is unknown.
- Tests: `an_ip_target_is_served_only_by_the_holder_in_its_own_network`, `a_private_address_is_one_node_per_network_it_is_read_in`, `only_a_private_address_depends_on_the_network_it_is_read_in`, `a_range_covers_exactly_the_addresses_inside_it_in_its_own_network`, `a_private_address_seen_by_an_interface_the_scan_has_not_found_is_counted_not_guessed`.

### Canonical GCP network keys
GCP APIs return network references in three forms: full URLs on GCE instances, relative paths on Cloud SQL, and bare names (`default`) on GKE clusters. Keying `GcpComputeNetwork` by raw API strings split one VPC into multiple nodes. `projector/gcp.rs::canonical_network` normalises all references to `projects/P/global/networks/N`.

### Transit gateway and peering connectors stay hubs
Expanding a transit gateway with $k = 4{,}000$ attachments into $k^2$ pairwise network joins cost 4.7 s and 1.2 GB per derivation in release builds (running multiple times per reconcile). Keeping connectors as hubs in `derive/networks.rs` (`O(k)`) reduced derivation to 0.5 ms.

### One connectivity collector
`DescribeVpcs`, `DescribeVpcPeeringConnections`, `DescribeTransitGateways`, `DescribeTransitGatewayAttachments`, `DescribeVpnGateways`, `DescribeCustomerGateways`, and `DescribeVpnConnections` stay one `connectivity` collector, like `networking`. Splitting it would keep VPCs and peerings when only transit gateway or VPN calls are denied, but `ReadOnlyAccess` grants all seven, and per-call collectors add collection variants for a partial-permission case.

### A tunnel needs both ends, each naming the other
A VPN's two ends each hold a public address (`ConnectsTo`) and name the other's as their peer (`RoutesTo`). Two simpler rules were wrong. Using a shared address as a mesh connector joined every network that names one gateway to each other, though hub-and-spoke through a gateway carries no spoke-to-spoke routing. Joining whichever network holds an address to whichever names it let one configured end join both networks: an Azure or GCP gateway holds its address with no connection at all, so an AWS connection naming it joined the networks while the Azure connection or GCP tunnel was missing, failed or being deleted. Now each tunnel end (AWS VPN connection, Azure or GCP gateway) pairs only with an end that names one of its addresses while it names one of that end's, and `Joins::joined` requires the two networks to sit on opposite ends of one tunnel. Ends pair one by one, so a gateway's two connections cannot combine into a tunnel. An Azure peering reaches a hub's gateway only when it projects the `RoutesTo` that `useRemoteGateways` produces; a plain peering stays a `ConnectsTo`.
- Tests: `a_tunnel_configured_at_one_end_joins_nothing`, `a_gateway_pairs_each_of_its_connections_on_its_own`, `two_networks_tunnelled_to_the_same_gateway_are_not_joined_to_each_other`, `only_a_peering_that_uses_remote_gateways_rides_the_hub_gateway`, `a_tunnel_to_a_gateway_the_scan_does_not_model_joins_nothing`, `a_private_address_resolves_across_a_vpn_tunnel`, property test `a_tunnel_joins_two_networks_only_when_each_end_names_the_other`.

### VPN lifecycle states are filtered alike on every cloud
AWS drew only `available` VPN gateways, customer gateways and connections, while Azure and GCP drew theirs in any state, so a connection being deleted or a tunnel that failed to build stayed on the graph. Azure gateways, local gateways and connections in `Deleting` or `Failed` now project nothing (`Updating` stays, as AWS `modifying` does, and is also the state a new Azure gateway is created in). GCP tunnels in `PROVISIONING`, `ALLOCATING_RESOURCES`, `DEPROVISIONING` or `FAILED` project nothing. GCP folds health into the same `status` field: `NO_INCOMING_PACKETS`, `NEGOTIATION_FAILURE` and other health values stay drawn (rule 6). A missing state is drawn. Since a tunnel needs both ends, skipping either end breaks its join.
- Tests: `an_azure_vpn_being_deleted_or_failed_is_not_projected`, `a_gcp_tunnel_is_projected_unless_it_is_being_built_or_torn_down`, `only_a_working_vpn_is_projected`, `a_tunnel_whose_far_end_is_going_away_resolves_nothing`.

### Dedicated circuits pair on link-scoped BGP addresses
The clouds' dedicated circuits share no public identifier. Circuit keys (ExpressRoute `serviceKey`, Partner Interconnect `pairingKey`) are handed to a connectivity partner, not to another cloud. Matching on customer ASN or colocation facility would be a guess, since one on-premises ASN is commonly announced to every cloud. What both ends of a circuit do share is the BGP link: each holds one address of a `/29`–`/31` and names the other. So a circuit is a link like a tunnel, and pairs under the same both-ends rule. The addresses are mostly link-local and repeat across circuits. Scoping them to the link's own subnet (`Node::link_ip`) keeps rule 5, and both ends compute that subnet from what they read. ExpressRoute exposes only prefixes; Azure documents that the customer side takes the first usable address and Microsoft the second, so the projector derives both. This joins clouds only over Cross-Cloud Interconnect. Circuits to an on-premises or partner router stay unjoined, because nothing in the scan models that router.
- Tests: `a_direct_connect_interface_and_an_interconnect_attachment_join_their_networks`, `circuits_that_end_at_routers_the_scan_does_not_model_join_nothing`, `an_expressroute_circuit_holds_the_second_address_of_each_peering_and_names_the_first`, `a_link_address_is_scoped_to_the_subnet_both_ends_share`, `a_private_address_resolves_across_a_dedicated_circuit`, property test `both_ends_of_a_link_read_their_addresses_in_the_subnet_they_share`.

### Direct Connect is its own collector
Direct Connect is a separate AWS service with its own client and IAM actions, so it is a `direct_connect` collector beside `connectivity`, the way every other AWS service is. A Direct Connect failure leaves VPCs, peerings and VPNs readable, and the reverse holds too. None of its four describe calls has an SDK paginator, so they page by hand and fail on any token they have already sent, which also stops a cursor that cycles. Associations are listed per Direct Connect gateway because the API requires a gateway, VGW or association to filter on.

Connections and virtual interfaces are regional, but Direct Connect gateways and their associations are account-wide: every regional endpoint returns the same list. Reading them in each region cost one gateway list plus one association call per gateway, per region, per scan (about 200 calls at 16 regions and 10 gateways), and any one throttled duplicate made AWS unreadable and held the whole estate. So `direct_connect_gateways` runs once per scan in the `account_wide` scope of `build_aws`, with Route 53 and CloudFront, which are account-wide for the same reason (Route 53's control plane allows five requests a second per account, and it was being read in every region at once). The scope is a future beside the regional ones, on the first region's client: it checks its own credentials and reports `global/credentials` or `global/<collector>` on failure, and its collections are tagged `global`. A virtual interface's `AttachedTo` its Direct Connect gateway is drawn after the parallel projection (`link_direct_connect_gateways`), because whether that gateway was skipped is now in another collection; it runs before `link_targets`, whose `held_addresses` reads the joins those edges make.
- Tests: `direct_connect_follows_every_page_of_connections_and_interfaces`, `direct_connect_gateways_ask_each_gateway_for_its_associations`, `direct_connect_refuses_a_page_token_that_repeats`, `direct_connect_refuses_a_page_token_that_comes_back`, `only_a_direct_connect_circuit_that_reaches_a_network_is_projected` (the interface and the skipped gateway it names arrive in different collections).

### One kind for every Azure gateway connection
`microsoft.network/connections` holds IPsec, VNet-to-VNet and ExpressRoute connections alike. Phase 2a named the kind `AzureVpnConnection` and skipped ExpressRoute ones. Drawing ExpressRoute under a separate kind would give one resource type two sources. So the kind is now `AzureGatewayConnection`, and its `peer` reference links an ExpressRoute connection to its circuit. ExpressRoute gateways were skipped for the same reason and are now drawn as `AzureVirtualNetworkGateway`.
- Test: `the_fixture_circuits_meet_on_their_link_addresses`.

### Link ends are found from the end, along directed steps
`Joins::of` first walked each `ends` path outward from every network, so every VPC on a transit gateway rescanned the gateway's whole edge list once per transit-gateway row (VPN and Direct Connect), even with no VPN connection or VIF on it: $O(k^2)$ at $k$ attachments, which undid the hub representation above. Each row now walks backwards from the nodes of its end kind, so the cost follows the ends that exist. Steps also name their edge's direction: matching either direction let a hub reach a spoke's own gateway back along the spoke's gateway-transit `RoutesTo`. A reference from a live resource no longer redraws a resource its own arm skips (an ExpressRoute circuit the provider deprovisioned, a Direct Connect gateway that is not `available`, an `UNPROVISIONED` interconnect, an Azure gateway or local gateway that is `Deleting` or `Failed`), and the empty gateway id DescribeVirtualInterfaces reports for an unset gateway draws nothing instead of a phantom gateway shared by every such VIF.
- Tests: `every_link_end_kind_belongs_to_one_row`, `a_hub_never_rides_the_gateway_of_a_network_that_routes_through_it`, `only_a_direct_connect_circuit_that_reaches_a_network_is_projected`, `an_expressroute_circuit_holds_the_second_address_of_each_peering_and_names_the_first`, `an_azure_vpn_being_deleted_or_failed_is_not_projected`, `an_interconnect_attachment_is_projected_unless_it_is_being_built_or_torn_down`.

### Azure ARG IP field casing and VM-to-NIC ownership
Azure Resource Graph spells `privateIPAddress` and `publicIPAddress` with uppercase `IP`; relying on `#[serde(rename_all = "camelCase")]` silently deserialised NICs with no addresses. Explicit `#[serde(rename)]` attributes are required. In addition, `AzureVirtualMachine` links to `AzureNetworkInterface` via `HasIp` (formerly `ConnectsTo`) so `derive::service` resolves NIC addresses up to the owning VM.

### Cloudflare has no per-resource IP addresses
Workers and Durable Objects run on shared anycast ranges fronting millions of tenants; KV, R2, and D1 sit behind runtime bindings or account-level API hosts. Only DNS `A`/`AAAA` records name IPs, and those are origin addresses linked via `ResolvesTo` (which confers no ownership).
- Test: `naming_an_address_is_not_holding_it`.

## Service Topology (`Edge::Serves`)

### ENI owner attribution rejects heuristics
- **Load balancers:** Rebuilding an ELB ARN from region and account was rejected because it guesses the partition (`aws` vs. `aws-cn`/`aws-us-gov`). `link_interface_owners` matches the `ELB app/…` or `ELB net/…` description suffix against collected load balancers after `project_parallel`.
- **Lambda:** `AWS Lambda VPC ENI-<function>-…` cannot be split from its UUID suffix without ambiguity, so no owner edge is drawn.
- **RDS:** Matching `RDSNetworkInterface` ENIs to DB instances by subnet and security group is a heuristic posing as identity. RDS links via DNS (`ConnectsTo -> GenericHostname(endpoint.address)`), while the unowned ENI retains its IPs so NLB IP-target paths still resolve to `AwsEc2Eni`.
- Test: `a_database_is_reached_through_its_endpoint_hostname`.

### Gate IP targets on `scanned_instances`
`link_instance_targets` skipped stopped instances, but IP targets naming a stopped instance's interface address still linked; with the instance excluded from the scan, the ENI resolved as its own owner (`NLB -Serves-> AwsEc2Eni`, `inferred`). `link_targets` now gates both target types after the parallel pass, using `derive::held_addresses` to skip addresses held locally or across a joined network by an interface attached to an unscanned instance.
- Tests: `a_stopped_ip_target_is_not_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred`, `an_ip_target_on_a_stopped_instances_interface_is_not_linked`.

### Diff-only `Serves` observations
Emitting all `Serves` observations on every reconcile turned every idle tick into a non-empty WebSocket patch. `DerivedObservations::changed` diffs against the previous reconcile state and emits only transitions.

### `Serves` naming, fan-out, and non-goals
- **Direction & name:** `A -Serves-> B` follows traffic direction (`ALB -Serves-> Instance`); `Reaches` was rejected as less descriptive.
- **Uncapped fan-out:** All secondary and IPv6 addresses become pivots, and owner fan-out per flow is uncapped until a real estate demonstrates a bottleneck.
- **Non-goals:** Request/response confirmation (port orientation is a heuristic; `Serves` does not prove B replied to A), inventing typed nodes from flow logs, typed external endpoints, and transitive `Serves` closure (belongs in queries, not stored edges).

## Rendering & Demo

### Warm-start a layout only after initial settle
Topology patches that arrived before the first layout settled warm-started from initial `(0, 0)` coordinates. Because coincident nodes exert zero repulsion, ForceAtlas2 settled immediately into a ±1 box (3 in 10 page loads at a 3 s poll). `GraphController` now warm-starts only after `laidOut` is set; earlier patches restart a cold layout.
- Test: `a patch that lands before the first layout settles still spreads the graph` (`render.spec.ts`).

### The demo produces only projector-emitted shapes
The demo's live change previously hand-linked `GenericHostname -ResolvesTo-> GenericIpAddress`—an ownerless-to-ownerless edge that no projector emits and `carry_forward` never expires. It now projects `fixtures::canary` through the Cloudflare projector.
- Tests: `the_live_change_is_a_shape_a_scan_produces`, `the_canary_record_toggles_by_parity`.
