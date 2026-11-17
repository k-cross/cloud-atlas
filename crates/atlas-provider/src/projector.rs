use atlas_graph::definition::{Edge, Node};
use atlas_graph::graph_builder::GraphBuilder;
use petgraph::graph::NodeIndex;
use rayon::prelude::*;

#[macro_export]
macro_rules! project_leaf {
    ($builder:expr, $items:expr, $field:ident, $variant:path) => {
        for item in $items {
            if let Some(id) = &item.$field {
                $builder.get_or_add_node($variant(id.as_str().into()));
            }
        }
    };
    ($builder:expr, $items:expr, $accessor:ident(), $variant:path, $parent:expr, $edge:expr) => {
        for item in $items {
            if let Some(id) = item.$accessor() {
                $builder.link_to($parent, $variant(id.into()), $edge);
            }
        }
    };
}

pub fn link_bgp_session(builder: &mut GraphBuilder, end: NodeIndex, held: Node, named: Node) {
    builder.link_to(end, held, Edge::ConnectsTo);
    builder.link_to(end, named, Edge::RoutesTo);
}

pub fn project_parallel<T: Sync>(
    builder: &mut GraphBuilder,
    items: &[T],
    project: impl Fn(&mut GraphBuilder, &T) + Sync + Send,
) {
    let sub_graphs: Vec<GraphBuilder> = items
        .par_iter()
        .map(|item| {
            let mut local = GraphBuilder::new();
            project(&mut local, item);
            local
        })
        .collect();

    for sub in &sub_graphs {
        builder.merge(&sub.graph);
    }
}
