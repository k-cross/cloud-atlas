//! Credential-free end-to-end check of the layout pipeline.
//!
//! Feed it any render snapshot; produce one without cloud credentials via
//! the main workspace's demo (`cargo run --example demo` writes
//! `multi_cloud_demo.json` from the Globex fixtures):
//!
//! ```sh
//! cargo run --example layout_demo -- ../multi_cloud_demo.json
//! ```

use atlas_layout::{ForceAtlas2, LayoutGraph, LayoutSettings};
use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: layout_demo <render-snapshot.json>");
    let json = std::fs::read_to_string(&path).expect("failed to read snapshot");
    let graph = LayoutGraph::from_json(&json).expect("failed to parse snapshot");
    println!(
        "Loaded {}: {} nodes, {} edges",
        path,
        graph.node_count(),
        graph.edge_count()
    );

    let mut layout = ForceAtlas2::new(graph, LayoutSettings::default());
    let iterations = 500;
    let start = Instant::now();
    layout.run(iterations);
    let elapsed = start.elapsed();

    let positions = layout.positions();
    assert!(
        positions.iter().all(|v| v.is_finite()),
        "layout produced non-finite coordinates"
    );
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for pair in positions.chunks_exact(2) {
        min_x = min_x.min(pair[0]);
        max_x = max_x.max(pair[0]);
        min_y = min_y.min(pair[1]);
        max_y = max_y.max(pair[1]);
    }
    println!(
        "{iterations} iterations in {elapsed:.2?} ({:.2?}/iter), final speed {:.4}",
        elapsed / iterations,
        layout.speed()
    );
    println!("Bounding box: [{min_x:.1}, {min_y:.1}] .. [{max_x:.1}, {max_y:.1}]");
}
