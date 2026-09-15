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

    fn estate(values: &[&str]) -> GraphBuilder {
        let mut builder = GraphBuilder::new();
        for value in values {
            builder.get_or_add_node(Node::ip(value));
        }
        builder
    }

    #[test]
    fn an_address_links_into_every_range_that_covers_it() {
        let mut builder = estate(&["198.51.0.0/16", "198.51.100.0/24", "198.51.100.10"]);
        link(&mut builder);

        assert!(covered(&builder, "198.51.100.0/24", "198.51.100.10"));
        assert!(covered(&builder, "198.51.0.0/16", "198.51.100.10"));
    }

    #[test]
    fn an_address_outside_every_range_is_left_alone() {
        let mut builder = estate(&["198.51.100.0/24", "203.0.113.10"]);
        link(&mut builder);

        assert!(!covered(&builder, "198.51.100.0/24", "203.0.113.10"));
        assert_eq!(builder.graph.edge_count(), 0);
    }

    #[test]
    fn a_range_is_never_linked_into_another_range() {
        let mut builder = estate(&["198.51.0.0/16", "198.51.100.0/24"]);
        link(&mut builder);

        assert_eq!(
            builder.graph.edge_count(),
            0,
            "a rule subsuming another rule says nothing about traffic"
        );
    }

    #[test]
    fn a_v4_address_never_matches_a_v6_range() {
        let mut builder = estate(&["2001:db8::/32", "192.0.2.10"]);
        link(&mut builder);

        assert_eq!(builder.graph.edge_count(), 0);
    }

    #[test]
    fn v6_ranges_cover_v6_addresses() {
        let mut builder = estate(&["2001:db8::/32", "2001:db8::10"]);
        link(&mut builder);

        assert!(covered(&builder, "2001:db8::/32", "2001:db8::10"));
    }

    #[test]
    fn a_range_written_with_host_bits_set_still_covers_its_network() {
        let mut builder = estate(&["198.51.100.10/24", "198.51.100.77"]);
        link(&mut builder);

        assert!(covered(&builder, "198.51.100.10/24", "198.51.100.77"));
    }

    #[test]
    fn a_value_that_is_not_an_address_is_skipped() {
        let mut builder = estate(&["198.51.0.0/16", "pl-1a2b3c4d"]);
        link(&mut builder);

        assert_eq!(builder.graph.edge_count(), 0);
    }

    #[test]
    fn linking_twice_does_not_duplicate_edges() {
        let mut builder = estate(&["198.51.100.0/24", "198.51.100.10"]);
        link(&mut builder);
        let once = builder.graph.edge_count();
        link(&mut builder);

        assert_eq!(builder.graph.edge_count(), once);
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
