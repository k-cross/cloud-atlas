//! Credential-free verification simulation.
//!
//! Projects the complete fake "Globex" multi-cloud environment
//! (`atlas_lib::fixtures`) onto a graph, renders it to a `.dot` file, and
//! prints a coverage report of every node and edge kind. Exits non-zero if
//! any kind fails to appear, so this doubles as a smoke test:
//!
//! ```sh
//! cargo run --example demo
//! dot -Tsvg multi_cloud_demo.dot -o demo.svg   # visualize
//! ```

use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::fixtures;
use petgraph::dot::Dot;
use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    println!("Projecting the fake Globex multi-cloud environment (no credentials needed)...\n");

    let builder = fixtures::build_graph();

    // Render the graph like the engine does (Display-formatted).
    let filename = "multi_cloud_demo.dot";
    let dot = format!("{}", Dot::with_config(&builder.graph, &[]));
    fs::write(filename, dot).expect("Failed to write dot file");

    // Tally every node and edge kind present in the graph.
    let mut node_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in builder.graph.node_weights() {
        *node_counts.entry(node.kind()).or_default() += 1;
    }
    let mut edge_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for edge in builder.graph.edge_weights() {
        *edge_counts.entry(edge.kind()).or_default() += 1;
    }

    println!("Node coverage ({} kinds):", Node::ALL_KINDS.len());
    for kind in Node::ALL_KINDS {
        match node_counts.get(kind) {
            Some(count) => println!("  ✓ {:<28} {}", kind, count),
            None => println!("  ✗ {:<28} MISSING", kind),
        }
    }

    println!("\nEdge coverage ({} kinds):", Edge::ALL_KINDS.len());
    for kind in Edge::ALL_KINDS {
        match edge_counts.get(kind) {
            Some(count) => println!("  ✓ {:<28} {}", kind, count),
            None => println!("  ✗ {:<28} MISSING", kind),
        }
    }

    let missing_nodes: Vec<&&str> = Node::ALL_KINDS
        .iter()
        .filter(|k| !node_counts.contains_key(**k))
        .collect();
    let missing_edges: Vec<&&str> = Edge::ALL_KINDS
        .iter()
        .filter(|k| !edge_counts.contains_key(**k))
        .collect();

    println!(
        "\nGraph: {} nodes, {} edges. Saved to {}.",
        builder.graph.node_count(),
        builder.graph.edge_count(),
        filename
    );

    if missing_nodes.is_empty() && missing_edges.is_empty() {
        println!("All node and edge kinds are present.");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "COVERAGE INCOMPLETE — missing node kinds: {:?}, missing edge kinds: {:?}",
            missing_nodes, missing_edges
        );
        eprintln!("Extend src/fixtures.rs (and the projectors) until every kind appears.");
        ExitCode::FAILURE
    }
}
