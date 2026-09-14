pub mod aws;
pub mod azure;
pub mod cloudflare;
pub mod gcp;

use crate::Settings;
use crate::atlas::graph_builder::GraphBuilder;
use crate::cloud::definition::Provider as CloudProvider;
use rayon::prelude::*;

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

pub(crate) use project_leaf;

pub(crate) fn project_parallel<T: Sync>(
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

pub fn build(builder: &mut GraphBuilder, data: &CloudProvider, opts: &Settings) {
    match data {
        CloudProvider::AWS(aws_data) => aws::aws_projector(builder, aws_data, opts),
        CloudProvider::GCP(gcp_data) => gcp::gcp_projector(builder, gcp_data),
        CloudProvider::Azure(azure_data) => azure::azure_projector(builder, azure_data),
        CloudProvider::Cloudflare(cf_data) => cloudflare::cloudflare_projector(builder, cf_data),
    }
}
