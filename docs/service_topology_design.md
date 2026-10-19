# Service Topology from Observed Traffic

Collapse multi-hop control-plane and data-plane paths into direct service dependencies:

```
external traffic -> region -> alb/nlb -> instance / service / db / queue / lambda
```

Control-plane APIs report what is *wired*; flow logs report what is *used*. Joining them requires two parts: **address coverage** on every flow endpoint, and the derived **`Edge::Serves`**. Historical decisions and rejected alternatives live in [`docs/audit_findings.md`](audit_findings.md).

## 1. Address Coverage

> **Invariant:** Every resource that can appear as a flow endpoint projects its addresses into the graph.

### AWS

`cloud/amazon/network_interface.rs` pages `DescribeNetworkInterfaces`. Every ENI projects its private, public, and IPv6 addresses, subnet, VPC, security groups, and instance attachment (`Instance -HasIp-> Eni -AttachedTo-> Subnet`), deduplicating with `project_instance` via `Node::ip`.

Unambiguous interface descriptions link owners to their ENIs:

| `interface_type` | `description` | Owner |
|---|---|---|
| `load_balancer` / `network_load_balancer` | `ELB app/…` or `ELB net/…` | `AwsElbLoadBalancer` (ARN suffix match in `link_interface_owners`, post-parallel) |
| `nat_gateway` | `Interface for NAT Gateway nat-…` | `AwsEc2NatGateway` (projector arm) |
| `lambda` | `AWS Lambda VPC ENI-<function>-…` | None (UUID suffix is not unambiguously splittable) |
| `vpc_endpoint` | `VPC Endpoint Interface vpce-…` | None (no node kind) |
| `interface` | `RDSNetworkInterface` | None (reached via DNS; ENI retains IPs for IP-target NLBs) |

- **RDS:** Projects `AwsRdsDbInstance -ConnectsTo-> GenericHostname(endpoint.address)`, joining Route 53 records at the hostname pivot (`a_database_is_reached_through_its_endpoint_hostname`), while its unowned ENI still answers IP-based flows.
- **Stopped instances (`scanned_instances` gate):** `DescribeInstances` collects only running/pending instances, but stopped instances remain attached to ENIs and registered in target groups. Both `link_interface_owners` and `link_targets` gate on `scanned_instances` so they never invent bare instance nodes (`an_interface_attached_to_an_instance_the_scan_dropped_invents_no_instance`, `a_stopped_target_is_not_inferred`). `link_targets` also skips IP targets whose address resolves—locally or across a single joined network via `derive::held_addresses`—to an interface attached to an unscanned instance (`a_stopped_ip_target_is_not_inferred`, `a_stopped_ip_target_across_a_peering_is_not_inferred`, `an_ip_target_on_a_stopped_instances_interface_is_not_linked`).
  - *Known gap:* A stopped instance's ENI remains in the graph holding its address; a client still sending traffic to that IP derives a `confirmed` `Serves` edge to `AwsEc2Eni`.
- **Tier 1:** `AWS::EC2::NetworkInterface` Config items project via `project_interface`/`InterfaceFacts`, omitting deferred owner edges (`an_interface_event_produces_only_what_a_full_scan_would`, `a_balancer_interface_event_does_not_invent_the_balancer`).

### GCP

`GcpComputeInstance` links directly via `ConnectsTo` to each interface's `networkIP` and `ipv6Address` and each access config's `natIP` and `externalIpv6` (the address peers observe on egress). Forwarding rules and Cloud SQL (`settings.ipConfiguration.privateNetwork`) do the same. All private addresses and firewall ranges normalise their network scope to `projects/…/global/networks/…`.

### Azure

`microsoft.network/networkinterfaces` (`ipConfigurations`) and `microsoft.network/loadbalancers` (`frontendIPConfigurations`) share one projector helper (with explicit `#[serde(rename)]` for ARG's uppercase `IP` fields):

| Field | Edge |
|---|---|
| `privateIPAddress` | `-ConnectsTo-> GenericPrivateIpAddress` (scoped to `subnet.id`'s VNet) |
| `publicIPAddress.id` | `-HasIp-> AzurePublicIpAddress` |
| `subnet.id` | `-AttachedTo-> AzureSubnet` |

`AzureVirtualMachine -HasIp-> AzureNetworkInterface` lets `derive::service` resolve NIC addresses up to the VM.

### Cloudflare

Cloudflare resources have no dedicated L3 addresses (Workers/DO use shared anycast; KV/R2/D1 use runtime/API hosts). DNS `A`/`AAAA` records project `GenericHostname -ResolvesTo-> CloudflareDnsRecord -ResolvesTo-> GenericIpAddress` (private answers scoped to the zone); `ResolvesTo` confers no ownership (`naming_an_address_is_not_holding_it`).

## 2. `Edge::Serves`

Implemented in `atlas/derive/service.rs`. Collapses paths like:

```
AwsElbLoadBalancer -HasIp-> AwsEc2Eni -ConnectsTo-> 10.10.1.50
                                       -TrafficFlow-> 10.10.2.10
                                       <-ConnectsTo- AwsEc2Eni <-HasIp- AwsEc2Instance
```

into `AwsElbLoadBalancer -Serves-> AwsEc2Instance`, oriented in traffic direction.

### Provenance, Observations & Lifecycle

- **Two provenances, one edge kind:** Control-plane wiring produces `inferred`; observed traffic between resolved owners produces `confirmed`. When traffic lapses on a wired target, status drops back to `inferred` rather than deleting the edge.
- **Status beside the graph:** `Edge::Serves` is payload-free. Its `status` and `last_seen` (`0` when `inferred`, max confirming flow timestamp in `FlowIndex` when `confirmed`, never `packets`/`bytes`) travel in `derive::observations` on snapshots and `DerivedObservations::changed` on patches, filtered to edges present in the graph.
- **Stateless derivation:** Recomputed by `derive::all` after `carry_forward` and `FlowIndex::overlay` (`!Edge::is_projected()`, never emitted by Tier-1 adapters).

| Event | Lifecycle Result |
|---|---|
| Target deregistered, no active flow | Edge removed |
| Flow lapses, target still registered | Reverts to `inferred`; edge stays |
| Flow lapses, no control-plane chain (e.g. Instance → RDS) | Edge removed |
| Load balancer deleted | Edge removed with node |
| Provider unreadable, carried forward | Chain survives (`inferred`); `confirmed` expires with flow |

### Derivation Rules

- **Ownership (`address_owners`):** A pivot is owned by whatever `ConnectsTo` it, walking transitively up `HasIp` (`Instance -HasIp-> Eni -HasIp-> Eip`) with a cycle-safe visited set. Private addresses resolve in their reading network first, falling back to the single joined network that holds the address (`network_connectivity_design.md`). An unowned ENI owns itself (`Instance -Serves-> AwsEc2Eni(rds)`). `ResolvesTo` never confers ownership; unowned endpoints (internet, NAT peers) emit no edge.
- **Port orientation:** Because flow logs record requests and replies separately, `flow::orientation` treats the non-ephemeral (or lower) port as the service endpoint (falling back to packet direction when ambiguous) and `FlowStats` tracks a per-pair majority (`a_reply_does_not_reverse_the_relationship`).
- **Control-plane chains (`CHAINS`):** Sequences of `Node::kind()` validated against `ALL_KINDS`, walking projected edges only: `[AwsElbLoadBalancer, AwsElbTargetGroup, AwsEc2Instance | GenericPrivateIpAddress | GenericIpAddress]` (`an_ip_target_is_served_only_by_the_holder_in_its_own_network`). `link_targets` projects `instance` targets via `ConnectsTo`, `ip` targets via `RoutesTo` a VPC-scoped pivot, and skips Lambda/ALB targets.
- **Globex fixture coverage:** `cargo run --example demo` derives 11 `Serves` edges: ALB → web-01/web-02 (`confirmed`) and web-03 (`inferred`); NLB → RDS ENI (`inferred` via IP target); plus `confirmed` web-01 → web-02, web-01 → RDS ENI, NAT → web-02, web-01 → warehouse (across VPC peering), web-02 → shared-services ENI (across TGW), web-01 → Azure LB (public IP), and web-02 → GCE instance (NAT IP). The two private interconnect flows (web-01 → Azure VM NIC, web-02 → Cloud SQL) remain unresolved until connectivity Phase 2 (`a_private_address_seen_from_another_network_names_nothing_there`).

### Frontend Rendering

`Serves` edges style by status (`servesColor`/`servesSize` in `style.ts`, defaulting to `inferred` when no observation exists) and never animate packet beads (gated on `FLOW_KIND`). `GraphController::setServiceView` hides all non-`Serves` edges and isolated nodes while keeping the confirmed/inferred counter panel visible.

## 3. Next Steps

1. **Cross-cloud VPN & interconnect resolution** — resolve private hops across VPN tunnels, Direct Connect, ExpressRoute, and Cloud Interconnect ([`docs/network_connectivity_design.md`](network_connectivity_design.md) Phase 2).
2. **GCP & Azure control-plane chains** — model GCP backend services and Azure load-balancer backend pools so their `Serves` edges can be `inferred` before traffic arrives.
