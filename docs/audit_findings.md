# Audit findings

Resolved findings, kept as patterns to avoid reintroducing. Each one is pinned
by a test; the test name is the fastest way back to the reasoning.

## Tier 2 (flow logs)

### A notification is only deleted when its objects were actually read

`FlowLogQueue::receive` used to push every message's receipt handle onto the
delete list regardless of what happened in between, so a transient S3 failure
(503, throttle, reset) recorded `Unavailable`, returned no observations, and
*still* deleted the message — losing that flow-log object permanently, because
SQS has already forgotten it.

Tier 1 can delete unconditionally: the only step between receiving and deleting
is a local parse, which cannot fail transiently. Tier 2 puts a network fetch in
that gap, so it needs the stronger rule. `read_object` therefore returns
`Option`: `None` means "could not read, leave it on the queue", `Some` means
"read it" — including when the content was unusable. A `Malformed` object is
still deleted, because redelivering something that will not parse just replays
the failure.

The cost is that a message naming several objects, one of which fails, is
redelivered whole and the successful objects are observed twice. Flow
observations are keyed by endpoint pair, so only the volume counters
double-count, and those reset when the entry lapses. Losing the object outright
is worse.

Pinned by `an_object_that_could_not_be_read_leaves_its_notification_on_the_queue`
and `an_object_that_was_read_but_makes_no_sense_is_deleted_anyway`.

### The merge context is bounded by what the index kept, not by the batch

`ingest_flows` built its merge subgraph from `batch.observations` — every record
that arrived — while `FlowIndex` had already evicted the oldest of them on the
way in. One flow-log object can carry `MAX_RECORDS` (100,000) records against a
`DEFAULT_CAPACITY` of 10,000, so a single busy object wrote up to ten times the
budget in `GenericIpAddress` nodes and `TrafficFlow` edges into the live graph,
broadcast them to every connected client, and left them there until the next
reconciliation swept them.

That contradicts the ceiling `atlas::flow` claims for itself: capacity is meant
to bound how far the overlay can inflate the twin. `FlowIndex::context` is now a
method on `&self` and filters the batch by what the index still holds.

Pinned by `the_merge_context_never_exceeds_what_the_index_kept`.

### Eviction cuts on a tie without collapsing the whole index

`evict` retained on `stats.last_seen > cutoff`, which drops *every* entry
sharing the cutoff timestamp. Flow-log records carry whole-second precision, so
ties are the normal case rather than a corner one, and in the degenerate case —
a batch whose records all share one second — the cutoff equals every entry's
`last_seen` and the entire index is wiped, emitting a lapse for traffic observed
a moment earlier.

`eviction_cut` now returns the cutoff *and* a budget for entries at it, so
eviction lands on the low-water mark instead of undershooting it by however many
entries happened to share a second.

Pinned by `eviction_survives_a_batch_that_shares_one_timestamp`.

### Truncation cuts on a character boundary

`decompress` read a gzipped object with `read_to_string` and then
`String::truncate(limit)`. Both fail when `limit` falls inside a multi-byte
character: `read_to_string` rejects the whole object as invalid UTF-8, and
`truncate` panics — in a daemon. It now reads bytes, cuts at the boundary at or
below the limit, and decodes, so only genuinely non-text content is rejected.

Pinned by `a_cut_that_lands_inside_a_character_still_yields_the_part_before_it`.

## Tier 1 (event feed)

### An interface with no reported subnet attaches to nothing

When a Config item carries no `configuration.networkInterfaces`, the adapter
falls back to the relationships list, which gives interface *ids* and no
subnets. `project_instance` used to read that missing subnet as "use the
instance's own", which is wrong exactly where it matters: a second ENI is
usually placed in a *different* subnet on purpose, so the event produced
`ENI -> AttachedTo -> subnet-a` while the full scan read `subnet-b` off the
interface itself. That is the Tier-1 rule-1 flap — reconciliation deletes the
event's edge, the next event puts it back, forever.

`EniFacts { subnet_id: None }` now yields no `AttachedTo` edge at all. The ENI
node and its `HasIp` edge still land, because the scan produces those too, and
Tier 3 supplies the attachment within a poll interval. A missing edge is
self-healing; a wrong one never settles.

Pinned by `an_eni_from_relationships_alone_attaches_to_no_subnet` and, for the
containment rule generally, `an_instance_event_produces_only_what_a_full_scan_would`.
