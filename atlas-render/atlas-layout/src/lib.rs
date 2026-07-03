//! Force-directed layout engine for cloud-atlas graphs.
//!
//! This crate is deliberately independent of `atlas-lib`: it consumes the
//! versioned render snapshot JSON (`atlas.json`) that the library exports and
//! knows nothing about clouds, providers, or petgraph. Node identity is a
//! dense index into a flat interleaved `[x0, y0, x1, y1, ..]` position
//! buffer, which crosses the wasm boundary as a `Float32Array` without
//! translation.
//!
//! The algorithm is ForceAtlas2 (Jacomy et al. 2014): degree-weighted
//! repulsion (Barnes-Hut approximated above a size cutoff), linear or lin-log
//! edge attraction, gravity, and the adaptive global/local speed scheme from
//! the paper. Layout is deterministic: initial placement is a phyllotaxis
//! spiral and no randomness is used anywhere.

pub mod forceatlas2;
pub mod graph;
pub mod quadtree;

pub use forceatlas2::{ForceAtlas2, LayoutSettings};
pub use graph::{GraphError, GraphSnapshot, LayoutGraph, SNAPSHOT_VERSION};
