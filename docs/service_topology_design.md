# Service Topology from Observed Traffic

## Overview

The goal is to read a request path off the graph:

```
external traffic -> region -> alb/nlb -> instance / service / db / queue / lambda
```

The control plane says what is *wired*. Only flow logs say what is *used*. Cloud
Atlas has both signals today and still cannot draw that chain, because the two
never meet: the control plane describes resources, flow records describe
addresses, and almost nothing in the graph carries an address.

This document covers the two phases that close it — **address coverage**, so a
flow endpoint resolves to a resource at all, and **`Edge::Serves`**, a derived
edge that collapses the resulting path into one link between typed resources.

It builds directly on `network_inference_design.md` (the `GenericIpAddress`
pivot and the Tier-2 overlay) and on the derivation pattern established by
`atlas::containment`.

## 1. What already exists

The chain is not missing from the model. For AWS, `projector/aws.rs:137-195`
builds:

```
AwsRegion -Contains-> AwsElbLoadBalancer -ConnectsTo-> AwsElbTargetGroup -ConnectsTo-> AwsEc2Instance
```

`Edge::Contains` puts the load balancer in its region or VPC, listeners attach
target groups to their balancer, and `DescribeTargetHealth` attaches instances to
their target group. So `alb -> instance` is present whenever a scan runs.

What is missing is everything flow logs were supposed to add on top: whether any
of those targets actually receives traffic, and the hops *past* the target group
— instance to database, instance to queue, lambda to anything — that no target
group describes and no control-plane API returns.

## 2. Why flow logs cannot see it: the address-coverage gap

`cloud/amazon/flow_logs.rs:198` already reads `interface-id` off every record and
builds `Node::AwsEc2Eni(id)`. `flow::admissible` then drops it, because Tier-2
rule 2 admits only nodes the scan already found, and the scan only finds ENIs
that hang off a described instance:

```rust
// projector/aws.rs:60 — EniFacts comes from DescribeInstances' networkInterfaces
Some(EniFacts { id: eni.network_interface_id()?, subnet_id: eni.subnet_id() })
```

Every other network participant is invisible. An ALB's traffic rides its
per-subnet ENIs, and `DescribeLoadBalancers` never returns their addresses. The
same is true of NAT gateways, RDS, in-VPC Lambda, VPC endpoints and EKS pods. So
an ALB-to-instance record lands as

```
GenericIpAddress(10.10.1.50) -TrafficFlow-> GenericIpAddress(10.10.2.10)
```

with the load balancer side an orphan pivot attached to nothing. The traffic is
recorded correctly and attributes to nothing, which is exactly what the graph
looks like today.

This is not AWS-specific, and AWS is not even the only offender:

| Cloud | What is collected | What is dropped | Consequence |
|---|---|---|---|
| AWS | ENIs attached to a described instance (`aws.rs:60`), instance primary private IP (`aws.rs:530`) | every unattached ENI: ALB/NLB, NAT gateway, RDS, in-VPC Lambda, VPC endpoint, EKS pod; instance secondary IPs | LB and managed-service traffic orphans |
| GCP | `network_interfaces[].network` (`gcp.rs:40-49`) | `network_ip`, `access_configs[].nat_ip` | a GCP VM has **no address at all** |
| Azure | VMs and their NIC *ids* (`azure/provider.rs:158`) | `microsoft.network/networkinterfaces`, `microsoft.network/loadbalancers` | `AzureNetworkInterface` nodes exist but are never collected, so they carry no address; no LB frontends at all |
| Cloudflare | DNS record content | — | already resolves to addresses |

GCP forwarding rules (`gcp.rs:193`) and Azure public IPs (`azure.rs:117`) are the
only load-balancer-shaped things that currently expose an address, and only
because those APIs return it inline rather than hiding it behind an interface
object.

### The invariant

> **Every resource that can appear as a flow endpoint must carry its addresses in
> the graph.**

That is the whole precondition, and it is provider-agnostic. Each cloud satisfies
it differently; AWS is the worst case because it is the only one that hides a
managed service's addresses behind a separate interface resource.

## 3. Phase 1 — address coverage

### 3.1 AWS: a `DescribeNetworkInterfaces` collector (the keystone)

One collector unlocks the rest. `DescribeNetworkInterfaces` returns every ENI in
the region rather than only instance-attached ones, and each carries exactly what
is needed:

| Field | What it gives |
|---|---|
| `network_interface_id` | the literal key every flow record carries as `interface-id` |
| `private_ip_addresses[]`, `association.public_ip` | the pivot a flow's `srcaddr`/`dstaddr` actually is |
| `interface_type` | the owner class: `load_balancer`, `network_load_balancer`, `nat_gateway`, `vpc_endpoint`, `lambda`, `interface`, `transit_gateway` |
| `description` | the owner id, in documented formats (below) |
| `attachment.instance_id` | the instance, when there is one |
| `subnet_id`, `vpc_id`, `groups` | the rest of the attachment, matching what `EniFacts` builds today |

**Owner stitching** comes from `description`, whose formats are stable and
documented:

| `interface_type` | `description` | Stitches to |
|---|---|---|
| `load_balancer` | `ELB app/<name>/<id>` | `AwsElbLoadBalancer` — the ARN ends `loadbalancer/app/<name>/<id>`, an exact suffix match |
| `network_load_balancer` | `ELB net/<name>/<id>` | same |
| `nat_gateway` | `Interface for NAT Gateway nat-…` | `AwsEc2NatGateway` |
| `vpc_endpoint` | `VPC Endpoint Interface vpce-…` | (no node kind yet) |
| `lambda` | `AWS Lambda VPC ENI-<function>-…` | `AwsLambdaFunction` |
| `interface` | `RDSNetworkInterface` | **nothing** — left unowned on purpose (§7); the DB is reached through its DNS endpoint instead |

Where the description does not name an owner unambiguously, the ENI is still a
node with its addresses; it simply has no owner edge. That is strictly better
than today, where it is not a node at all. **Never guess an owner from a
description we do not recognise** — an unowned ENI is an honest answer, an
invented edge is not.

This also settles an inconsistency noted separately: the private IP currently
hangs off the *instance* (`aws.rs:530`), not the interface that owns it, even
though architecture rule 2 makes the ENI the networking pivot and `interface-id`
the identity every flow record carries. With this collector the address attaches
to the ENI, and the instance-level edge stays only as a fallback for an instance
that reports no interfaces.

Two consequences beyond service topology:

- **Tier 1's Config ENI arm opens.** `cloud/amazon/events.rs` deliberately leaves
  ENI configuration items unmapped, and `CLAUDE.md` names this collector as the
  precondition: the arm was closed only because a Config item for a NAT gateway's
  ENI would have been a node no full scan produced. Once the scan produces it,
  the arm is safe.
- **`flow::admissible` starts admitting `AwsEc2Eni`.** The `resources` list on
  every `FlowObservation` becomes real attribution rather than dropped on the
  floor for everything that is not an instance.

### 3.2 GCP and Azure

Both are smaller:

- **GCP** — `gcp.rs:40-49` already iterates `network_interfaces`; read
  `network_ip` and `access_configs[].nat_ip` and link them as
  `GcpComputeInstance -ConnectsTo-> GenericIpAddress`. Forwarding rules already
  do this.
- **Azure** — `Node::AzureNetworkInterface` already exists
  (`definition.rs:123`) and VMs already link to NIC ids, but
  `microsoft.network/networkinterfaces` is not in the `azure_types!` list
  (`azure/provider.rs:72`), so those nodes are created by reference and never
  populated. Adding the type there makes the compiler demand a mapping arm;
  `ipConfigurations[].privateIPAddress` then gives the address the existing edge
  was always pointing at. `microsoft.network/loadbalancers` needs a new node kind
  as well as a type-list entry.

### 3.3 As built (1a)

`cloud/amazon/network_interface.rs` paginates `DescribeNetworkInterfaces`;
`AmazonCollection::AmazonNetworkInterfaces` carries the result, and the
projector arm gives each interface its addresses (every
`private_ip_addresses[]` entry and its association, the top-level private and
public address, and every `ipv6_addresses[]` entry), its subnet and VPC, its
security groups, and its instance attachment — the same
`Instance -HasIp-> Eni -AttachedTo-> Subnet` shape `project_instance` already
built, so the two collections merge on identity rather than disagreeing.

Owner stitching landed for the two cases a description names unambiguously:

- **NAT gateway** in the projector arm, since `Interface for NAT Gateway nat-…`
  yields the id `Node::AwsEc2NatGateway` is already keyed by.
- **Load balancer** in `link_interface_owners`, which runs *after*
  `project_parallel`. This edge cannot be built inside the parallel pass:
  projection runs each collection into its own `GraphBuilder`, so the interface
  arm cannot see the balancer nodes, and neither collection carries the other's
  key. The pass builds `app/<name>/<id>` → ARN from the balancer collection and
  matches interface descriptions against it, emitting nothing when there is no
  match. Reconstructing the ARN from region + `owner_id` was the alternative and
  was rejected: it guesses the partition, and a wrong guess creates a second,
  fictional balancer node.

The **attached instance** edge turned out to belong in that same pass, for a
different reason, and a review caught it after 1a landed. `DescribeInstances` is
filtered to running/pending; `DescribeNetworkInterfaces` is not, and a *stopped*
instance keeps its interfaces attached with `attachment.instanceId` populated.
Drawing the edge from the attachment alone therefore called `get_or_add_node` on
an instance the scan deliberately excluded — creating a node with no AZ, tags or
security groups that is nevertheless the same `kind()` as a live one, and which
Tier 3 would then keep confirming rather than collecting. (`events.rs` maps every
non-terminated state change to `Created`, so the stopped instance had a live feed
re-asserting it too.) The edge now requires the instance to be in the scan's own
instance collection, which costs nothing: a running instance reports its
interfaces and `project_instance` has already drawn the same edge. The residual
case it still buys — a described instance that reports no interfaces — is pinned
by `an_interface_owns_its_attachment_once_the_scan_confirms_the_instance`, and
the invention by
`an_interface_attached_to_an_instance_the_scan_dropped_invents_no_instance`.

Lambda, VPC endpoint and RDS interfaces are **left unowned** — Lambda's
description embeds a function name that cannot be split from its UUID suffix
without assuming the format, VPC endpoints have no node kind, and RDS was decided
against in §7. All three are still nodes carrying their addresses, which is what
the flow overlay needs.

`Node::ip` doing the canonicalising means the ENI arm and the instance arm can
both assert the same address without producing two pivots, and `add_edge`'s dedup
absorbs the overlap — no explicit reconciliation between the two collections.

### 3.4 What Phase 1 alone buys

Flow endpoints resolve. The demo's `alb -> instance` traffic attributes to the
load balancer. Node freshness lands on every network participant rather than only
on instances. **No new edge kind, no wire change.** Phase 2 is optional on top,
and worth doing separately so the address work can land and be verified on its
own.

### 3.5 As built (1b)

The RDS projector arm now reads `endpoint.address` and emits
`AwsRdsDbInstance -ConnectsTo-> GenericHostname(endpoint)`, the linkage §7
settled on in place of stitching an `RDSNetworkInterface` to its database by
subnet and security group. The collector already returned the endpoint
(`collector_tests.rs::rds_db_instances` asserts it deserializes); nothing
consumed it.

`Node::hostname` is what makes the seam close: the Route 53 record set that
points at the endpoint and the database itself both canonicalise to the same
pivot, so `RecordSet -ConnectsTo-> GenericHostname <-ConnectsTo- DbInstance`
falls out of `GraphBuilder`'s node identity with no reconciliation pass. The
Globex fixture carries a `db.globex.io` CNAME to the endpoint so that seam is
exercised credential-free, and
`a_database_is_reached_through_its_endpoint_hostname` pins both halves.

The RDS interface stays unowned, as §7 requires — 1a's
`an_interface_names_the_owner_its_description_actually_identifies` still asserts
no database points at `eni-globex-rds-1a`, and the IP path keeps working through
that interface's own address for the NLB-fronted case DNS cannot see.

### 3.6 As built (1c)

The Tier-1 Config arm §3.1 left closed is open: `AWS::EC2::NetworkInterface`
maps to `Node::AwsEc2Eni`, and its context goes through the projector's own
`project_interface`, now sharing an `InterfaceFacts` seam with the SDK path
exactly as `project_instance`/`InstanceFacts` does. The precondition was the
whole reason it was closed — a Config item for a NAT gateway's ENI used to be a
node no full scan produced, and 1a made the scan produce it.

`use_aws_resource` already filters `AWS::EC2::NetworkInterface` out of the
catch-all, so the typed node is the entire event rather than a duplicate beside
an `AwsConfigResource`.

Both edges `link_interface_owners` defers are held back here too, under Tier-1
rule 1 and for the same reason: each needs a collection the event path does not
have. A balancer ARN cannot be rebuilt from `ELB app/<name>/<id>` without
guessing the partition, and an attachment cannot be trusted without the scan's
own running/pending instance list — a stopped instance's Config item would
otherwise invent the instance node through the live feed instead of the scan.
The interface still arrives with its addresses, subnet, security groups and
NAT-gateway owner; the rest is added by the next reconciliation.
`a_balancer_interface_event_does_not_invent_the_balancer` pins that, and
`an_interface_event_produces_only_what_a_full_scan_would` pins the containment
rule by projecting the same interface both ways — which is also what keeps the
two paths from drifting apart again.

## 4. Phase 2 — `Edge::Serves`

### 4.1 The problem Phase 1 leaves

With addresses in place the path is correct and unreadable:

```
AwsElbLoadBalancer -HasIp-> AwsEc2Eni -ConnectsTo-> 10.10.1.50
                                       -TrafficFlow-> 10.10.2.10
                                       <-ConnectsTo- AwsEc2Eni <-HasIp- AwsEc2Instance
```

Five hops through two untyped pivots to say "the load balancer sends traffic to
this instance". Fine for a query, useless as a picture, and it is the picture
that is the goal.

### 4.2 One edge, two provenances

`Edge::Serves` is a **derived** edge between two typed resources, source →
target, in the direction traffic moves: the source directs traffic to the target,
and the target serves it. `ALB -Serves-> Instance`;
`Instance -Serves-> RdsDbInstance`. This matches the arrow direction in the goal
chain above.

It is derived from two independent kinds of evidence, and deliberately **one edge
kind rather than two**:

- **The control-plane chain.** `ALB -ConnectsTo-> TargetGroup -ConnectsTo->
  Instance` means the wiring exists. Derives `Serves` with status `inferred`.
- **Observed traffic.** A `TrafficFlow` edge whose two address pivots resolve to
  those same two resources means the wiring is *used*. Promotes the status to
  `confirmed`.

Keeping them as one edge is what makes the interesting transition expressible: a
flow that lapses drops the edge back to `inferred` rather than deleting it,
because the control plane still says the target is wired up. An ALB target that
is registered and receiving nothing is a different and more useful statement than
an ALB target that does not exist — and with two separate edge kinds it would
read as a deletion.

### 4.3 Status lives beside the graph, never on the edge

Architecture rule 6: `Node` and `Edge` are the graph's identity types, hashed and
diffed by value. A status field inside `Edge::Serves` would make every
inferred→confirmed transition a *different edge* — duplicating past
`GraphBuilder::add_edge`'s dedup and emitting a remove-then-add of the same
`edge_key` in every reconciliation patch. This is the same reasoning that made
`Edge::TrafficFlow` payload-free.

So `Serves` is payload-free too, and its status rides the **existing observation
channel**, keyed by `edge_key` exactly as `FlowStats` is:

```rust
RenderObservation { key, last_seen, packets: None, bytes: None, status }
```

with `status` taking the new values `"inferred"` and `"confirmed"`. The wire
shape is unchanged, so **`SNAPSHOT_VERSION` does not move** — v3 already carries
`observations` on the snapshot and `observations`/`expired` on the patch, and
`GraphPatch` already transports a metadata change for an edge that did not itself
change. `packets`/`bytes` are already `Option` and are omitted, because a
`Serves` edge summarises possibly many flows and summing their packets would
report the same traffic more than once — the same reason `Liveness` on a node has
no volume. `last_seen` composes as a maximum and is carried: for `confirmed` it
is the newest confirming flow; for `inferred` it is `0`, meaning never observed.

### 4.4 Derived, so owned by no tier

The same answer `containment` gave. A `Serves` edge is a pure function of what is
in the graph at the moment it is computed, so it needs no retention, no TTL and
no state of its own. It is recomputed wherever a graph is finalized, after
`carry_forward` and after `FlowIndex::overlay`, and every lifecycle falls out of
the ordinary Tier-3 differ:

| What happens | Result |
|---|---|
| Target deregistered from the target group | control-plane chain gone; if no flow either, edge removed by the differ |
| Flow lapses, target still registered | status drops to `inferred`; **edge stays** |
| Flow lapses, no control-plane chain (instance → RDS) | edge removed by the differ |
| Load balancer deleted | its node goes, so the edge goes |
| Provider unreadable, resources carried forward | chain is carried, so `inferred` is recomputed; `confirmed` is not, because a dark provider says nothing about traffic |

That last row is why `Edge::is_projected()` must return **false** for `Serves`,
exactly as it does for `TrafficFlow` and `Covers`. The exhaustive match in
`definition.rs` forces the decision when the variant is added.

Correspondingly, **no event adapter may emit `Serves`** (Tier-1 rule 1): an
adapter may only produce what the full-scan projector produces, and `Serves` is
produced by the derivation pass, not the projector. An adapter that emitted one
would have it removed at the next reconciliation and re-added by the next event,
forever.

### 4.5 Deriving it

Two inputs, one pass, both O(V + E):

**Address ownership.** Build `HashMap<NodeIndex, Vec<NodeIndex>>` from address
pivots to the typed resources that claim them, by scanning edges once. An address
legitimately has more than one owner — an Azure public IP is claimed by
`AzurePublicIpAddress` *and* reachable through the load balancer that fronts it —
so this is a `Vec`, and derivation fans out across owners. Fan-out is deliberately
uncapped (§7): a shared address producing many `Serves` edges is a real
observation about the estate, and the map should be instrumented and measured
before anything is trimmed.

**Observed pairs.** For each `TrafficFlow` edge, look up both endpoints'
owners and emit `Serves` for every resolved pair, with no cap on fan-out (§7). An endpoint that resolves to
nothing produces nothing — the external side of a flow is a `GenericIpAddress`
with no owner by design (Tier-2 rule 2), so internet traffic yields no `Serves`
edge, and a NAT gateway talking to ten thousand external addresses produces zero.
The derivation is naturally bounded by typed-to-typed pairs.

**Control-plane chains** are per-provider knowledge and cannot be inferred
generically, so they are a declarative table of node-kind path patterns:

```
[AwsElbLoadBalancer, AwsElbTargetGroup, AwsEc2Instance]     // real today
[GcpComputeForwardingRule, <backend service>, GcpComputeInstance]
[<azure load balancer>, <backend pool>, AzureVirtualMachine]
```

matched against `Node::kind()`. The engine stays generic; only the table is
provider-specific, and a pattern naming a kind that does not exist fails against
`ALL_KINDS`. Only the AWS row is complete at the start: GCP has no backend-service
kind and Azure has neither a load-balancer nor a backend-pool kind
(`definition.rs` has `AzureNetworkInterface` but no `AzureLoadBalancer`), so
until those land GCP and Azure `Serves` edges come from observed traffic alone.

### 4.6 Where it runs

`containment::link` is already called from four finalisation points
(`poll::reconcile`, `AtlasEngine::install`, `fixtures::build_graph`,
`examples/demo.rs`), and the Edge exhaustiveness guard caught the fourth only
because it was missed first. A second derivation pass makes that fragility worse,
so both should move behind one entry point:

```rust
atlas::derive::all(&mut builder);   // containment, then service topology
```

Ordering inside is fixed and stated there: containment first (it adds edges but
no nodes, and service derivation does not read `Covers`).

## 5. Rendering

No wire change, so the frontend work is small and additive:

- `EDGE_COLORS` gains `Serves`.
- `graph.ts:116` gates observation styling behind `kind === FLOW_KIND`; it needs
  a second arm so a `Serves` edge takes a colour from its status —
  `confirmed` solid and saturated, `inferred` dimmed or dashed. The distinction
  between wired and used is the entire point of the edge and has to be visible.
- The traffic layer must **not** animate packets along a `Serves` edge. It
  carries no `packets`, and `TrafficLayer` already keys off `FLOW_KIND`, so this
  holds as long as that check is not loosened.
- A `Serves` edge and the `TrafficFlow` edges it summarises are both in the
  graph. A filter or layer toggle is probably wanted so the service view can be
  read without the address-level traffic underneath it, but that is a rendering
  concern, not a modelling one.

## 6. Deliberately not built

- **Request/response semantics.** Flow logs record packets between addresses.
  They do not prove a request was served, a call succeeded, or which side
  initiated a long-lived connection. `Serves` means traffic moved in that
  direction; it does not mean B answered A.
- **Typed nodes invented from flow records.** Unchanged from Tier-2 rule 2. If
  Phase 1 has not given a resource an address, its traffic stays on a generic
  pivot rather than being guessed at.
- **The external end.** `external -> region` in the goal chain stays a
  `GenericIpAddress`/`GenericHostname` pivot by design. DNS and the load
  balancer's own public addresses already connect it; nothing should turn an
  internet address into a typed node.
- **Transitive service paths.** `A -Serves-> B -Serves-> C` is a query over the
  derived edges, not another derived edge. Materialising transitive closure would
  explode with estate size for no information the graph does not already hold.

## 7. Decisions

All five questions this document opened are settled.

- **Naming and direction — `Serves`, source → target, in the direction traffic
  moves.** `ALB -Serves-> Instance`; `Instance -Serves-> RdsDbInstance` reads as
  "the database serves the instance". `Reaches` was the more neutral alternative
  and was not chosen; it says less.
- **RDS ENIs stay unowned; the DNS endpoint carries the relationship.** A
  `RDSNetworkInterface` description does not name its database, so stitching by
  subnet and security group would be a heuristic dressed as an identity. Instead
  the projector gains
  `AwsRdsDbInstance -ConnectsTo-> GenericHostname(endpoint.address)` — which it
  does not build today at all — and the ENI remains a node carrying its
  addresses, with no owner edge.

  **The IP path must keep working regardless**, because DNS does not cover every
  case. Cross-region and cross-VPC access to RDS is commonly fronted by an NLB
  targeting IPs, and there the client resolves the *balancer's* name, never the
  database's: the flow records read client → NLB ENI and NLB ENI → RDS ENI, with
  the database's own endpoint appearing nowhere. Because the RDS ENI is still a
  node with its addresses, that second hop attributes to a real interface even
  though it cannot be attributed to the database. DNS is the better primary
  linkage, not the only one, and nothing in Phase 1 may assume otherwise.
- **Secondary and IPv6 addresses: take them all, then measure.** Every address on
  an ENI becomes a pivot and a resource with several fans out ownership. Do not
  pre-emptively trim; instrument the address-ownership map and measure on a real
  estate before deciding whether fan-out is a problem.
- **AWS first, across clouds second.** Getting AWS relationships right is worth
  more than breadth, so Phase 1b (GCP `network_ip`/`nat_ip`, Azure NICs and load
  balancers) waits. Only the AWS control-plane path pattern is real until then,
  and GCP/Azure `Serves` edges — where their resources have addresses at all —
  come from observed traffic alone. Asymmetric on purpose.
- **Data first; optimise later.** The derivation is O(V + E) and runs on every
  reconciliation. Build it to record everything it finds, measure it, and only
  then decide what to bound. Concretely this removes the proposed cap on how many
  owners one address pivot may fan out to: a shared or anycast address producing
  a burst of `Serves` edges is a real observation about the estate, and dropping
  it to protect a budget nobody has measured would be losing data to solve a
  problem we have not confirmed exists.

## 8. Phased plan

| Phase | Deliverable | Status |
|---|---|---|
| **1a** | AWS `DescribeNetworkInterfaces` collector; `AwsEc2Eni` carries its addresses and owner edges; instance private IP moved to the ENI with an instance-level fallback | **Done** — `cloud/amazon/network_interface.rs`, the `AmazonNetworkInterfaces` projector arm, and `link_load_balancer_interfaces` |
| **1b** | AWS `AwsRdsDbInstance -ConnectsTo-> GenericHostname(endpoint)` — not built today, and the primary way the database is reached | **Done** — the `AmazonRds` projector arm, §3.5 |
| **1c** | Tier-1 Config ENI arm opened, now that the scan produces the node | **Done** — `AWS::EC2::NetworkInterface` in `events.rs`, via `project_interface`/`InterfaceFacts`, §3.6 |
| **1d** | GCP `network_ip`/`nat_ip`; Azure `networkinterfaces` + `loadbalancers` — deferred behind AWS (§7) | Deferred |
| **2a** | `atlas::derive` — one entry point, `containment` moved behind it | Planned |
| **2b** | `Edge::Serves` from observed traffic; status on the observation channel | Planned |
| **2c** | Control-plane path patterns; `inferred` → `confirmed` promotion | Planned |
| **2d** | Frontend: status-coloured `Serves`, service-view filter | Planned |
