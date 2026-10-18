use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::{mask_to_prefix, parse_address};
use petgraph::graph::NodeIndex;
use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;

pub fn link(builder: &mut GraphBuilder) {
    let mut ranges: HashMap<(Option<&str>, IpAddr, u8), Vec<NodeIndex>> = HashMap::new();
    let mut addresses: Vec<(NodeIndex, Option<&str>, IpAddr)> = Vec::new();

    for index in builder.graph.node_indices() {
        let (scope, value) = match &builder.graph[index] {
            Node::GenericIpAddress(value) => (None, value),
            Node::GenericPrivateIpAddress { network, address } => (Some(network.as_ref()), address),
            _ => continue,
        };
        match parse_address(value) {
            Some((addr, Some(prefix))) => {
                let Some(network) = mask_to_prefix(&addr, prefix) else {
                    continue;
                };
                ranges
                    .entry((scope, network, prefix))
                    .or_default()
                    .push(index);
            }
            Some((addr, None)) => addresses.push((index, scope, addr)),
            None => {}
        }
    }

    if ranges.is_empty() || addresses.is_empty() {
        return;
    }

    // Probing only the widths actually present keeps this proportional to the
    // rules an estate really wrote, not to the 33/129 a family could express.
    let widths: BTreeSet<u8> = ranges.keys().map(|(_, _, prefix)| *prefix).collect();

    let mut links: Vec<(NodeIndex, NodeIndex)> = Vec::new();
    for (address_index, scope, addr) in &addresses {
        for width in &widths {
            let Some(network) = mask_to_prefix(addr, *width) else {
                continue;
            };
            let Some(covering) = ranges.get(&(*scope, network, *width)) else {
                continue;
            };
            links.extend(covering.iter().map(|range| (*range, *address_index)));
        }
    }

    for (range, address) in links {
        builder.add_edge(range, address, Edge::Covers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETWORK: &str = "vpc-globex";

    fn covered(builder: &GraphBuilder, range: &Node, address: &Node) -> bool {
        builder.has_edge(range, address, &Edge::Covers)
    }

    #[test]
    fn a_rule_is_confirmed_by_the_traffic_it_permitted() {
        let mut builder = GraphBuilder::new();
        let sg = builder.get_or_add_node(Node::AwsEc2SecurityGroup("sg-web".into()));
        builder.link_to(sg, Node::ip("203.0.113.0/24", NETWORK), Edge::RoutesTo);
        let peer = builder.get_or_add_node(Node::ip("10.0.0.1", NETWORK));
        builder.link_to(peer, Node::ip("203.0.113.77", NETWORK), Edge::TrafficFlow);

        link(&mut builder);

        assert!(covered(
            &builder,
            &Node::ip("203.0.113.0/24", NETWORK),
            &Node::ip("203.0.113.77", NETWORK)
        ));
    }

    #[test]
    fn a_private_range_covers_only_the_addresses_of_its_own_network() {
        let mut builder = GraphBuilder::new();
        let sg = builder.get_or_add_node(Node::AwsEc2SecurityGroup("sg-web".into()));
        let range = Node::ip("10.20.0.0/16", NETWORK);
        builder.link_to(sg, range.clone(), Edge::RoutesTo);
        let local = Node::ip("10.20.0.5", NETWORK);
        let elsewhere = Node::ip("10.20.0.5", "vpc-elsewhere");
        for address in [&local, &elsewhere] {
            let peer = builder.get_or_add_node(Node::ip("203.0.113.9", NETWORK));
            builder.link_to(peer, address.clone(), Edge::TrafficFlow);
        }

        link(&mut builder);

        assert!(covered(&builder, &range, &local));
        assert!(!covered(&builder, &range, &elsewhere));
    }
}
