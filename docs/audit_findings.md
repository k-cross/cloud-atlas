# Audit findings

Resolved findings, kept as patterns not to reintroduce. Each is pinned by the named test.

## Tier 2 (flow logs)

### Delete a notification only after its objects were read

`FlowLogQueue::receive` deleted every message even when the S3 read failed transiently, losing the object for good. Tier 1 can delete unconditionally because only a local parse sits between receive and delete; Tier 2 has a network fetch there. `read_object` returns `Option`: `None` leaves the message queued, `Some` deletes it — including unparseable content (`Malformed`), which would only fail again.

Exception: a 404/410 object is gone, not retryable, and retrying would re-observe its siblings forever and inflate their counters. It is reported `Malformed` and the message released. Auth failures still retry.

Trade-off: a partially failed message is redelivered whole, so the objects that did read are double-counted until their entries lapse. That beats losing data.

Tests: `an_object_that_could_not_be_read_leaves_its_notification_on_the_queue`, `an_object_that_was_read_but_makes_no_sense_is_deleted_anyway`, `an_object_that_is_gone_releases_its_notification`.

### Bound the merge context by what the index kept

`ingest_flows` built its merge subgraph from every arriving record, although `FlowIndex` had already evicted some. One object (`MAX_RECORDS` = 100,000) against `DEFAULT_CAPACITY` (10,000) could write ten times the budget into the live graph. `FlowIndex::context` now filters by what the index holds.

Test: `the_merge_context_never_exceeds_what_the_index_kept`.

### Evict on a tie without emptying the index

`evict` kept `last_seen > cutoff`, dropping every entry *at* the cutoff. Flow records have one-second precision, so ties are normal, and a single-second batch wiped the whole index. `eviction_cut` returns the cutoff plus a budget for entries at it.

Test: `eviction_survives_a_batch_that_shares_one_timestamp`.

### Truncate on a character boundary

`read_to_string` + `String::truncate(limit)` rejected or panicked when the limit fell inside a multi-byte character. `decompress` now reads bytes, cuts at a boundary, then decodes.

Test: `a_cut_that_lands_inside_a_character_still_yields_the_part_before_it`.

## Tier 1 (event feed)

### An interface with no reported subnet attaches to nothing

With no `configuration.networkInterfaces`, the adapter reads interface ids from relationships, which carry no subnet. `project_instance` used to assume the instance's subnet, which is wrong for secondary ENIs and made event and scan disagree forever. `EniFacts { subnet_id: None }` now yields no `AttachedTo`; the scan supplies it. A missing edge heals, a wrong one flaps.

Tests: `an_eni_from_relationships_alone_attaches_to_no_subnet`, `an_instance_event_produces_only_what_a_full_scan_would`.

### Prune ordering on a tie without emptying the table

`EventApplier::prune` had the same strict-cutoff bug as `evict`; an emptied table let a redelivered create resurrect a deleted resource. `prune` now shares `eviction_cut`/`survives` with the flow index.

Test: `pruning_keeps_a_batch_that_shares_one_timestamp`.

## Tier 3 (reconciliation)

### Carry an edge by its owner, not by the pivot it touches

`carry_forward` kept an edge if *either* endpoint was held, and ownerless pivots count as held whenever anything references them. So a healthy provider's stale edge into a shared pivot came back every tick while another provider was down. An edge is now carried only when every endpoint that has an owner is held.

Test: `carry_forward_lets_a_healthy_source_delete_its_edge_to_a_shared_pivot`.

### No projected edge may join two ownerless pivots

The owner rule above passes vacuously for an edge with no owned endpoint, and such an edge anchors its own endpoints. Cloudflare projected `hostname -ResolvesTo-> target`, so while *any* provider was unreadable a healthy zone's repointed or deleted records kept their old answers. DNS now reads `hostname -ResolvesTo-> record -ResolvesTo-> target` for Cloudflare and Route 53 alike (the record-to-name edge was also `RoutesTo`, which means traffic).

Tests: `every_projected_edge_has_an_owned_endpoint`, `carry_forward_lets_a_healthy_dns_provider_repoint_and_delete_records`.
