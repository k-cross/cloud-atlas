use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::{mask_to_prefix, parse_address};
use petgraph::graph::NodeIndex;
use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;

pub fn link(builder: &mut GraphBuilder) {
    let mut ranges: HashMap<(IpAddr, u8), Vec<NodeIndex>> = HashMap::new();
    let mut addresses: Vec<(NodeIndex, IpAddr)> = Vec::new();

    for index in builder.graph.node_indices() {
        let Node::GenericIpAddress(value) = &builder.graph[index] else {
            continue;
        };
        match parse_address(value) {
            Some((addr, Some(prefix))) => {
                let Some(network) = mask_to_prefix(&addr, prefix) else {
                    continue;
                };
                ranges.entry((network, prefix)).or_default().push(index);
            }
            Some((addr, None)) => addresses.push((index, addr)),
            None => {}
        }
    }

    if ranges.is_empty() || addresses.is_empty() {
        return;
    }

    // Probing only the widths actually present keeps this proportional to the
    // rules an estate really wrote, not to the 33/129 a family could express.
    let widths: BTreeSet<u8> = ranges.keys().map(|(_, prefix)| *prefix).collect();

    let mut links: Vec<(NodeIndex, NodeIndex)> = Vec::new();
    for (address_index, addr) in &addresses {
        for width in &widths {
            let Some(network) = mask_to_prefix(addr, *width) else {
                continue;
            };
            let Some(covering) = ranges.get(&(network, *width)) else {
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

    fn covered(builder: &GraphBuilder, range: &str, address: &str) -> bool {
        builder.has_edge(&Node::ip(range), &Node::ip(address), &Edge::Covers)
    }

    #[test]
    fn a_rule_is_confirmed_by_the_traffic_it_permitted() {
        let mut builder = GraphBuilder::new();
        let sg = builder.get_or_add_node(Node::AwsEc2SecurityGroup("sg-web".into()));
        builder.link_to(sg, Node::ip("203.0.113.0/24"), Edge::RoutesTo);
        let peer = builder.get_or_add_node(Node::ip("10.0.0.1"));
        builder.link_to(peer, Node::ip("203.0.113.77"), Edge::TrafficFlow);

        link(&mut builder);

        assert!(covered(&builder, "203.0.113.0/24", "203.0.113.77"));
    }
}
