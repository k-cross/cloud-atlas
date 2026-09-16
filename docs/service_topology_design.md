# Service Topology from Observed Traffic

## Goal

Read a request path off the graph:

```
external traffic -> region -> alb/nlb -> instance / service / db / queue / lambda
```

The control plane says what is *wired*; flow logs say what is *used*. They only meet if flow endpoints (addresses) resolve to resources. Two phases close that gap: **address coverage**, and **`Edge::Serves`**, a derived edge that collapses the resulting path into one link between typed resources. Builds on `network_inference_design.md`.

## 1. Address coverage

> **Invariant: every resource that can appear as a flow endpoint carries its addresses in the graph.**

Before phase 1, AWS only knew instance-attached ENIs and instance primary IPs, so load balancer, NAT, RDS, Lambda and VPC endpoint traffic landed on orphan pivots. AWS is the hardest case because it hides managed services' addresses behind a separate interface resource.

### AWS (done)

`cloud/amazon/network_interface.rs` pages `DescribeNetworkInterfaces`. Each interface gets every private, public and IPv6 address, its subnet, VPC, security groups and attachment — the same `Instance -HasIp-> Eni -AttachedTo-> Subnet` shape `project_instance` builds, so the two merge by identity. `Node::ip` canonicalising means both can assert the same address without producing two pivots.

Owner edges come only from descriptions that name an owner unambiguously:

| `interface_type` | `description` | Owner |
|---|---|---|
| `load_balancer` / `network_load_balancer` | `ELB app/…` or `ELB net/…` | `AwsElbLoadBalancer` (ARN suffix match, in `link_interface_owners`) |
| `nat_gateway` | `Interface for NAT Gateway nat-…` | `AwsEc2NatGateway` (projector arm) |
| `lambda` | `AWS Lambda VPC ENI-<function>-…` | none — the name can't be split from its suffix without guessing |
| `vpc_endpoint` | `VPC Endpoint Interface vpce-…` | none — no node kind |
| `interface` | `RDSNetworkInterface` | none — see below |

An unrecognised description means no owner edge, never a guessed one. The load balancer edge runs after `project_parallel`, since each collection projects into its own builder; rebuilding the ARN from region and account was rejected because it guesses the partition.

**RDS** is reached by DNS instead: `AwsRdsDbInstance -ConnectsTo-> GenericHostname(endpoint.address)`, which meets Route 53 records at the same pivot. Stitching an RDS ENI to its database by subnet and security group would be a heuristic posing as identity. The ENI still carries its addresses, which matters when an NLB fronts RDS by IP and the database's name never appears in traffic. Pinned by `a_database_is_reached_through_its_endpoint_hostname`.

**Stopped instances.** `DescribeInstances` is filtered to running/pending, but stopped instances stay attached to ENIs and registered in target groups. Edges from those mentions are gated on `scanned_instances`, or they would invent bare instance nodes. Pinned by `an_interface_attached_to_an_instance_the_scan_dropped_invents_no_instance` and `a_stopped_target_is_not_inferred`.

**Tier 1.** `AWS::EC2::NetworkInterface` Config items now map through `project_interface`/`InterfaceFacts`, which was unsafe until the scan produced every ENI. Both deferred owner edges are omitted from events, since each needs a collection the event lacks. Pinned by `an_interface_event_produces_only_what_a_full_scan_would` and `a_balancer_interface_event_does_not_invent_the_balancer`.

### GCP and Azure (deferred)

Getting AWS right came first.

- **GCP** — instances drop `network_ip` and `access_configs[].nat_ip`; link them as `GcpComputeInstance -ConnectsTo-> GenericIpAddress`. Forwarding rules already carry their address.
- **Azure** — `AzureNetworkInterface` exists but `microsoft.network/networkinterfaces` isn't in `azure_types!`, so NICs are created by reference with no address. Adding it yields `ipConfigurations[].privateIPAddress`. Load balancers need a new kind as well.

## 2. `Edge::Serves`

With addresses in place the path is correct but unreadable:

```
AwsElbLoadBalancer -HasIp-> AwsEc2Eni -ConnectsTo-> 10.10.1.50
                                       -TrafficFlow-> 10.10.2.10
                                       <-ConnectsTo- AwsEc2Eni <-HasIp- AwsEc2Instance
```

`Serves` collapses it to `ALB -Serves-> Instance`, directed the way traffic moves (`Instance -Serves-> RdsDbInstance` reads "the database serves the instance"). Implemented in `atlas/derive/service.rs`.

### One kind, two provenances

- **Wiring:** a control-plane chain (`ALB -ConnectsTo-> TargetGroup -ConnectsTo-> Instance`) → status `inferred`.
- **Traffic:** a `TrafficFlow` whose endpoints resolve to the two resources → `confirmed`.

One kind makes the useful transition expressible: a lapsed flow drops a registered target back to `inferred` instead of deleting it. "Wired and idle" differs from "gone". Internally, observed pairs and chain matches fill the same map; an empty flow list *is* `inferred`.

### Status beside the graph

A status field would make every transition a different edge (architecture rule 6), so `Serves` is payload-free and its status rides the observation channel keyed by `edge_key`: `last_seen` (newest confirming flow, `0` when inferred) and `status`, no `packets`/`bytes` since one edge may summarise many flows. `SNAPSHOT_VERSION` stays 3. Snapshots use `derive::observations`; patches use `DerivedObservations::changed`, which sends only what moved (resending everything made every idle tick a patch). Observations are filtered to edges the graph holds. Confirmation requires the flow to still be in the index, since an evicted flow's edge can outlive its record.

### Derived, owned by no tier

`Serves` is a pure function of graph and index, recomputed by `derive::all` after `carry_forward` and the overlay, so the differ handles every lifecycle:

| Event | Result |
|---|---|
| Target deregistered, no flow | edge removed |
| Flow lapses, target still registered | back to `inferred`, edge stays |
| Flow lapses, no chain (e.g. instance → RDS) | edge removed |
| Load balancer deleted | edge removed with the node |
| Provider unreadable, carried forward | chain survives → `inferred`; `confirmed` does not |

Hence `Edge::is_projected()` is false for `Serves`, and no event adapter may emit it.

### Derivation

- **Ownership.** A pivot is owned by what `ConnectsTo` it, resolved transitively up `HasIp` (a visited set breaks cycles), so `Eni -Serves-> Eni` becomes `ALB -Serves-> Instance` and an Elastic IP held by an interface (`Instance -HasIp-> Eni -HasIp-> Eip`) isn't a second owner. An interface nothing holds owns itself — `Instance -Serves-> AwsEc2Eni(rds)` is the honest answer when the database can't be named. `ResolvesTo` never confers ownership, so a DNS record is never a `Serves` endpoint; this is an edge-kind rule, not a blocklist, which keeps the pass provider-agnostic.
- **Observed pairs.** For each flow, emit `Serves` for every pair of resolved owners, uncapped (below). An unowned endpoint (internet addresses, a NAT gateway's peers) produces nothing.
- **Orientation.** A flow log writes request and reply as separate records, so packet direction draws every relationship both ways. `FlowObservation` carries ports; `flow::orientation` treats the non-ephemeral (or lower) port as the service, falls back to packet direction when ambiguous, and `FlowStats` keeps a per-pair majority. Pinned by `a_reply_does_not_reverse_the_relationship`.
- **Chains.** `CHAINS` lists `Node::kind()` sequences, checked against `ALL_KINDS`; the matcher walks projected edges only, so it can't read its own output. Only `[AwsElbLoadBalancer, AwsElbTargetGroup, AwsEc2Instance]` exists; GCP (backend service) and Azure (load balancer, backend pool) lack the kinds, so their `Serves` edges come from traffic alone.
- **Targets.** Projected by `target_type`: `instance` via `link_instance_targets`; `ip` as `RoutesTo` a pivot (`ConnectsTo` would make the group an address holder); Lambda/ALB targets skipped.

The Globex fixture derives eight edges, printed by `cargo run --example demo`: the ALB to web-01 and web-02 (`confirmed`) and to web-03 (registered and idle, `inferred`); plus `confirmed` web-01 → web-02, web-01 → the RDS ENI, NAT → web-02, and two cross-cloud hops (to an Azure public IP and a GCP SQL instance).

### Rendering

`Serves` is coloured and sized by status (`servesColor`/`servesSize`), with `inferred` styling whenever no observation is present. The traffic layer keys off `FLOW_KIND`, so it never animates `Serves`. `GraphController::setServiceView` hides everything except `Serves` edges and their endpoints — hiding, not dimming, since the summarised hops are exactly the clutter being removed. The panel shows confirmed/inferred counts and stays visible while the view is on, even if it becomes empty.

## 3. Decisions

- **Name and direction:** `Serves`, in traffic direction. `Reaches` said less.
- **RDS:** DNS linkage primary, the ENI path kept for IP-fronted access.
- **All addresses, then measure:** secondary and IPv6 addresses all become pivots. Owner fan-out is uncapped — a shared or anycast address producing many edges is a real observation, and no budget has been measured to justify dropping it.
- **AWS first.** GCP/Azure wait.
- **Not built:** request/response semantics (ports are a heuristic; `Serves` doesn't mean B answered A); typed nodes from flow records; typed external endpoints; transitive `Serves` closure (a query, not an edge).

## 4. Phases

| Phase | Deliverable | Status |
|---|---|---|
| 1a | `DescribeNetworkInterfaces`; ENIs carry addresses and owners | Done |
| 1b | RDS → endpoint hostname | Done |
| 1c | Tier-1 Config ENI arm | Done |
| 1d | GCP and Azure addresses | Deferred |
| 2a | `atlas::derive` single entry point | Done |
| 2b | `Serves` from traffic | Done |
| 2c | Control-plane chains, `inferred` status | Done |
| 2d | Frontend status styling, service view | Done |
| — | IP targets in chains (`[LB, TargetGroup, <address>]`), e.g. NLB → RDS | Not started |
