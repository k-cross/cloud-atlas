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

## Networking & Pivots

### A private address is unique only inside one network
`GenericIpAddress` was originally keyed by address string alone, merging `10.0.1.5` or default-VPC `172.31.0.0/16` addresses across every VPC, account, region, and cloud. Private ranges now project as `GenericPrivateIpAddress { network, address }` via `Node::ip(value, network)`. Flow records resolve private IPs in the capturing interface's network and are dropped (`Malformed`) when the network is unknown.
- Tests: `an_ip_target_is_served_only_by_the_holder_in_its_own_network`, `a_private_address_is_one_node_per_network_it_is_read_in`, `only_a_private_address_depends_on_the_network_it_is_read_in`, `a_range_covers_exactly_the_addresses_inside_it_in_its_own_network`, `a_private_address_seen_by_an_interface_the_scan_has_not_found_is_counted_not_guessed`.

### Canonical GCP network keys
GCP APIs return network references in three forms: full URLs on GCE instances, relative paths on Cloud SQL, and bare names (`default`) on GKE clusters. Keying `GcpComputeNetwork` by raw API strings split one VPC into multiple nodes. `projector/gcp.rs::canonical_network` normalises all references to `projects/P/global/networks/N`.

### Transit gateway and peering connectors stay hubs
Expanding a transit gateway with $k = 4{,}000$ attachments into $k^2$ pairwise network joins cost 4.7 s and 1.2 GB per derivation in release builds (running multiple times per reconcile). Keeping connectors as hubs in `derive/networks.rs` (`O(k)`) reduced derivation to 0.5 ms.

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
