use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::export::{RenderObservation, render_snapshot_with};
use atlas_lib::fixtures;
use petgraph::dot::Dot;
use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    println!("Projecting the fake Globex multi-cloud environment (no credentials needed)...\n");

    let mut builder = fixtures::topology();
    let observed = fixtures::observed();
    observed.overlay(&mut builder);

    let filename = "multi_cloud_demo.dot";
    let dot = format!("{}", Dot::with_config(&builder.graph, &[]));
    fs::write(filename, dot).expect("Failed to write dot file");

    let snapshot = render_snapshot_with(&builder.graph, observed.observations());
    let json = serde_json::to_string(&snapshot).expect("Failed to serialize render snapshot");
    fs::write("multi_cloud_demo.json", json).expect("Failed to write json file");

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

    let all = observed.observations();
    let (flows, resources): (Vec<&RenderObservation>, Vec<&RenderObservation>) =
        all.iter().partition(|o| o.packets.is_some());

    println!(
        "\nTraffic overlay ({} flows, {} resources heard from):",
        observed.flow_count(),
        observed.resource_count()
    );
    for flow in &flows {
        let (src, dst) = endpoints(&flow.key);
        println!(
            "  {} {:>34} -> {:<34} {:>7} packets  {:>9}",
            marker(flow.status),
            src,
            dst,
            flow.packets.unwrap_or_default(),
            human_bytes(flow.bytes.unwrap_or_default()),
        );
    }
    println!("\nHeard from ({}):", resources.len());
    for resource in &resources {
        println!("  {} {}", marker(resource.status), label(&resource.key));
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
        "\nGraph: {} nodes, {} edges, {} observations. Saved to {}.",
        builder.graph.node_count(),
        builder.graph.edge_count(),
        all.len(),
        filename
    );

    if !missing_nodes.is_empty() || !missing_edges.is_empty() {
        eprintln!(
            "COVERAGE INCOMPLETE — missing node kinds: {:?}, missing edge kinds: {:?}",
            missing_nodes, missing_edges
        );
        eprintln!("Extend src/fixtures.rs (and the projectors) until every kind appears.");
        return ExitCode::FAILURE;
    }

    if flows.is_empty() || resources.is_empty() {
        eprintln!("TRAFFIC OVERLAY EMPTY — fixtures::flows() produced no observations.");
        return ExitCode::FAILURE;
    }

    println!("All node and edge kinds are present, and traffic is flowing.");
    ExitCode::SUCCESS
}

fn marker(status: &str) -> &'static str {
    match status {
        "accepted" => "✓",
        "rejected" => "✗",
        "mixed" => "±",
        _ => "·",
    }
}

fn label(key: &str) -> &str {
    key.split_once('#').map_or(key, |(_, rest)| rest)
}

fn endpoints(flow_key: &str) -> (&str, &str) {
    let pair = flow_key.split_once('|').map_or(flow_key, |(_, rest)| rest);
    match pair.split_once("->") {
        Some((src, dst)) => (label(src), label(dst)),
        None => (label(pair), ""),
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}
