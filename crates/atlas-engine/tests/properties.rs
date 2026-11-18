use atlas_graph::collection::CollectionSource;
use atlas_graph::definition::{Edge, Node};
use atlas_graph::graph_builder::GraphBuilder;
use atlas_graph::patch::{carry_forward, diff};
use petgraph::graph::Graph;
use proptest::prelude::*;

fn source() -> impl Strategy<Value = CollectionSource> {
    prop_oneof![
        Just(CollectionSource::Aws),
        Just(CollectionSource::Gcp),
        Just(CollectionSource::Azure),
        Just(CollectionSource::Cloudflare),
    ]
}

proptest! {
    #[test]
    fn carry_forward_restores_unreadable_territory_and_never_a_healthy_source_s(
        unreadable in prop::collection::hash_set(source(), 0..=4),
        dropped in prop::collection::hash_set(source(), 0..=4),
    ) {
        let live = atlas_engine::fixtures::build_graph().graph;

        let mut trimmed = live.clone();
        trimmed.retain_nodes(|g, i| {
            g[i].owner().is_none_or(|owner| !dropped.contains(&owner))
        });

        let mut next = GraphBuilder::new();
        next.merge(&trimmed);
        carry_forward(&mut next, &live, &unreadable);

        let owned = |graph: &Graph<Node, Edge>, source: CollectionSource| {
            graph
                .node_weights()
                .filter(|n| n.owner() == Some(source))
                .count()
        };

        for provider in [
            CollectionSource::Aws,
            CollectionSource::Gcp,
            CollectionSource::Azure,
            CollectionSource::Cloudflare,
        ] {
            if unreadable.contains(&provider) {
                prop_assert_eq!(
                    owned(&next.graph, provider),
                    owned(&live, provider),
                    "an unreadable source must keep everything it owned: {}",
                    provider
                );
            } else if dropped.contains(&provider) {
                prop_assert_eq!(
                    owned(&next.graph, provider),
                    0,
                    "a healthy source's deletions must go through: {}",
                    provider
                );
            }
        }

        if dropped.is_empty() {
            prop_assert!(
                diff(&live, &next.graph).is_empty(),
                "a scan that dropped nothing leaves nothing for carry_forward to change"
            );
        }
    }
}
