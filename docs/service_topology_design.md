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

An IP target names the instance by address instead, and its interface keeps that address while stopped. The interface has no holder once the instance is gone, so it would resolve as its own owner — the rule meant for RDS and Lambda interfaces — and the target group would read as serving a standalone interface. `link_targets` therefore skips an IP target held by an interface attached to an instance the scan dropped, in the group's own VPC or, when nothing holds it there, in the one joined network that does. Pinned by `a_stopped_ip_target_is_not_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred` and `an_ip_target_on_a_stopped_instances_interface_is_not_linked`.

Known gap: the interface itself stays in the graph with its address (the event path must agree with the scan on it, and "attached to a stopped instance" is state rule 6 keeps off the node). A client still sending to that address — retrying a stopped backend — gets a `confirmed` `Serves` edge to the interface, since the traffic is real and the interface holds the address.

**Tier 1.** `AWS::EC2::NetworkInterface` Config items now map through `project_interface`/`InterfaceFacts`, which was unsafe until the scan produced every ENI. Both deferred owner edges are omitted from events, since each needs a collection the event lacks. Pinned by `an_interface_event_produces_only_what_a_full_scan_would` and `a_balancer_interface_event_does_not_invent_the_balancer`.

### GCP (done)

GCE has no separate interface resource, so an instance holds its addresses directly: `GcpComputeInstance -ConnectsTo-> GenericIpAddress`, for each interface's `networkIP` and `ipv6Address` and each access config's `natIP` and `externalIpv6`. A private one is read in the interface's network, as are a firewall's ranges, an internal forwarding rule's address and a Cloud SQL private IP (`settings.ipConfiguration.privateNetwork`); all are cut to `projects/…/global/networks/…`, since instances report a full URL and Cloud SQL a path. The NAT address matters most — it is the one a peer's flow log records for egress. Forwarding rules and Cloud SQL instances already carried theirs.

### Azure (done)

`microsoft.network/networkinterfaces` and `microsoft.network/loadbalancers` join `azure_types!`. A NIC's `ipConfigurations` and a balancer's `frontendIPConfigurations` are the same shape, so one projector helper reads both:

| Field | Edge |
|---|---|
| `privateIPAddress` | `-ConnectsTo-> GenericPrivateIpAddress`, in the VNet of `subnet.id` |
| `publicIPAddress.id` | `-HasIp-> AzurePublicIpAddress` |
| `subnet.id` | `-AttachedTo-> AzureSubnet` |

ARG spells both address fields `IP`, not `Ip`, so each carries an explicit `serde(rename)`; camelCase alone would deserialise a NIC that holds nothing. `AzureLoadBalancer` is the new kind — without it a balancer's frontend address belongs to a bare `AzurePublicIpAddress`, which is an address object, not something that serves.

**`AzureVirtualMachine -HasIp-> AzureNetworkInterface`**, where it used to be `ConnectsTo`. `derive::service` resolves an address up through its holders, so this is what makes a flow to a NIC's address read as the machine rather than the interface — the same shape as `Instance -HasIp-> Eni`. Backend pools are still unmodelled, so Azure has no control-plane chain and its `Serves` edges come from traffic alone.

### Cloudflare (nothing to carry)

Cloudflare is absent from this phase because it has no per-resource address to collect, not because it was skipped.

- **Workers and Durable Objects** run on shared anycast. The addresses Cloudflare publishes are network-wide ranges fronting every zone on the platform, so pinning one to a worker or a zone would be the same guess the RDS ENI case rejects — worse, since one address really does front millions of tenants.
- **KV, R2 and D1** are reached through the Workers runtime or an account-level API host. There is no per-namespace or per-bucket endpoint for a flow record to name.
- **The one address Cloudflare knows** is the `A`/`AAAA` content of a DNS record, and it is already in the graph: `GenericHostname -ResolvesTo-> CloudflareDnsRecord -ResolvesTo-> GenericIpAddress`. That is an *origin* address, held by whatever actually answers on it — often an ENI in the same graph. A private answer is read in the zone, since a public record cannot say which network's `10.x` it means, so it reaches no holder. `ResolvesTo` confers no ownership (architecture rule 5), so the record never becomes a `Serves` endpoint; the fixture's `v6.globex.io` names `2001:db8::10`, which is also a flow source, and draws no edge. Pinned by `naming_an_address_is_not_holding_it`.

So the invariant holds for Cloudflare vacuously: no Cloudflare resource can be the thing a flow endpoint resolves to. If that changes — Workers with dedicated egress IPs, or Spectrum's per-application addresses — the resource would carry them with `ConnectsTo` like any other.

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

- **Ownership.** A pivot is owned by what `ConnectsTo` it, resolved transitively up `HasIp` (a visited set breaks cycles), so `Eni -Serves-> Eni` becomes `ALB -Serves-> Instance` and an Elastic IP held by an interface (`Instance -HasIp-> Eni -HasIp-> Eip`) isn't a second owner. A private address carries the network it was read in, so an IP target or a flow record finds the holder in its own VPC, never a resource elsewhere that reuses the range. One nothing holds there resolves across a modelled join (peering, a transit gateway) when exactly one joined network holds it (`network_connectivity_design.md`). An interface nothing holds owns itself — `Instance -Serves-> AwsEc2Eni(rds)` is the honest answer when the database can't be named. `ResolvesTo` never confers ownership, so a DNS record is never a `Serves` endpoint; this is an edge-kind rule, not a blocklist, which keeps the pass provider-agnostic.
- **Observed pairs.** For each flow, emit `Serves` for every pair of resolved owners, uncapped (below). An unowned endpoint (internet addresses, a NAT gateway's peers) produces nothing.
- **Orientation.** A flow log writes request and reply as separate records, so packet direction draws every relationship both ways. `FlowObservation` carries ports; `flow::orientation` treats the non-ephemeral (or lower) port as the service, falls back to packet direction when ambiguous, and `FlowStats` keeps a per-pair majority. Pinned by `a_reply_does_not_reverse_the_relationship`.
- **Chains.** `CHAINS` lists `Node::kind()` sequences, checked against `ALL_KINDS`; the matcher walks projected edges only, so it can't read its own output. Three AWS rows exist: `[AwsElbLoadBalancer, AwsElbTargetGroup, AwsEc2Instance]`, and the same ending in `GenericPrivateIpAddress` or `GenericIpAddress` for IP targets (an ownerless endpoint resolves through `address_owners`, so an IP target names whatever holds the address in the group's VPC, and nothing if nothing does). Pinned by `an_ip_target_is_served_only_by_the_holder_in_its_own_network`. GCP (backend service) and Azure (a backend pool under `AzureLoadBalancer`) lack the kinds, so their `Serves` edges come from traffic alone.
- **Targets.** Projected by `target_type` in `link_targets`, after the parallel pass: `instance` gated on `scanned_instances`; `ip` as `RoutesTo` a pivot in the group's VPC (`ConnectsTo` would make the group an address holder), gated on the address not belonging to a dropped instance's interface; Lambda/ALB targets skipped.

The Globex fixture derives eleven edges, printed by `cargo run --example demo`: the ALB to web-01 and web-02 (`confirmed`) and to web-03 (registered and idle, `inferred`); the NLB to the RDS ENI (`inferred`, via its IP target group); plus `confirmed` web-01 → web-02, web-01 → the RDS ENI, NAT → web-02, two hops across joins (web-01 → the warehouse in `vpc-globex-data` over the peering, web-02 → the shared-services endpoint over the transit gateway), and two cross-cloud hops over public addresses — web-01 to the Azure load balancer (its frontend public IP) and web-02 to the GCE instance (its NAT address). The fixture's two private hops over the interconnect, web-01 to the Azure VM's NIC and web-02 to the Cloud SQL instance, derive nothing: their addresses are read in `vpc-globex`, where nothing holds them (`a_private_address_seen_from_another_network_names_nothing_there`). They return with network connectivity phase 2.

### Rendering

`Serves` is coloured and sized by status (`servesColor`/`servesSize`), with `inferred` styling whenever no observation is present. The traffic layer keys off `FLOW_KIND`, so it never animates `Serves`. `GraphController::setServiceView` hides everything except `Serves` edges and their endpoints — hiding, not dimming, since the summarised hops are exactly the clutter being removed. The panel shows confirmed/inferred counts and stays visible while the view is on, even if it becomes empty.

## 3. Decisions

- **Name and direction:** `Serves`, in traffic direction. `Reaches` said less.
- **RDS:** DNS linkage primary, the ENI path kept for IP-fronted access.
- **All addresses, then measure:** secondary and IPv6 addresses all become pivots. Owner fan-out is uncapped — a shared or anycast address producing many edges is a real observation, and no budget has been measured to justify dropping it.
- **AWS first.** GCP and Azure followed in 1d, once the AWS shape had proven itself.
- **Not built:** request/response semantics (ports are a heuristic; `Serves` doesn't mean B answered A); typed nodes from flow records; typed external endpoints; transitive `Serves` closure (a query, not an edge).

## 4. Phases

| Phase | Deliverable | Status |
|---|---|---|
| 1a | `DescribeNetworkInterfaces`; ENIs carry addresses and owners | Done |
| 1b | RDS → endpoint hostname | Done |
| 1c | Tier-1 Config ENI arm | Done |
| 1d | GCP and Azure addresses | Done |
| 2a | `atlas::derive` single entry point | Done |
| 2b | `Serves` from traffic | Done |
| 2c | Control-plane chains, `inferred` status | Done |
| 2d | Frontend status styling, service view | Done |
| 2e | IP targets in chains (`[LB, TargetGroup, <address>]`), e.g. NLB → RDS | Done |
| 2f | Private addresses scoped to their network; IP targets gated on `scanned_instances` | Done |
| 2g | Resolution across peering and transit gateways (`network_connectivity_design.md` phase 1) | Done |
| — | Resolution across VPNs and interconnects (connectivity phase 2) | Next |
