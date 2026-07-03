//! ForceAtlas2 (Jacomy, Venturini, Heymann, Bastian — PLoS ONE 2014) with
//! the paper's adaptive speed scheme. One `step()` is one physics iteration;
//! the caller (native test, wasm bridge, or a future worker loop) owns the
//! cadence, so a browser can interleave steps with rendering frames.

use crate::graph::LayoutGraph;
use crate::quadtree::QuadTree;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Below this many nodes exact pairwise repulsion is cheaper than building
/// the quadtree every iteration.
const BARNES_HUT_CUTOFF: usize = 64;

#[derive(Debug, Clone)]
pub struct LayoutSettings {
    /// Repulsion strength (kr). Larger spreads the graph out.
    pub repulsion: f32,
    /// Pull toward the origin, scaled by node mass. Keeps disconnected
    /// components from drifting off to infinity.
    pub gravity: f32,
    /// Gravity proportional to distance instead of constant — compacts very
    /// sparse graphs.
    pub strong_gravity: bool,
    /// Attraction grows with log(1 + d) instead of d — tightens clusters.
    pub lin_log: bool,
    /// Barnes-Hut opening criterion. 0 = exact, larger = faster and coarser.
    pub theta: f32,
    /// Use Barnes-Hut approximation for repulsion (above the size cutoff).
    pub barnes_hut: bool,
    /// Tolerance to swinging in the adaptive speed controller. Larger
    /// converges faster but less precisely.
    pub jitter_tolerance: f32,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            repulsion: 2.0,
            gravity: 1.0,
            strong_gravity: false,
            lin_log: false,
            theta: 0.9,
            barnes_hut: true,
            jitter_tolerance: 1.0,
        }
    }
}

pub struct ForceAtlas2 {
    graph: LayoutGraph,
    settings: LayoutSettings,
    /// Interleaved [x0, y0, x1, y1, ..] — the buffer handed across the wasm
    /// boundary as a Float32Array.
    positions: Vec<f32>,
    /// Forces of the current and previous iteration (same interleaving).
    /// Both are kept because the speed controller measures per-node
    /// "swinging" — the disagreement between successive force directions.
    forces: Vec<f32>,
    prev_forces: Vec<f32>,
    masses: Vec<f32>,
    speed: f32,
    speed_efficiency: f32,
}

impl ForceAtlas2 {
    pub fn new(graph: LayoutGraph, settings: LayoutSettings) -> Self {
        let n = graph.node_count();
        // Deterministic phyllotaxis spiral: evenly spread, no two nodes
        // coincide, and identical input always yields the identical layout.
        let mut positions = Vec::with_capacity(2 * n);
        for i in 0..n {
            let radius = 10.0 * (i as f32).sqrt();
            let angle = (i as f32) * 2.399_963_2; // golden angle in radians
            positions.push(radius * angle.cos());
            positions.push(radius * angle.sin());
        }
        let masses = (0..n).map(|i| graph.mass(i)).collect();
        Self {
            graph,
            settings,
            positions,
            forces: vec![0.0; 2 * n],
            prev_forces: vec![0.0; 2 * n],
            masses,
            speed: 1.0,
            speed_efficiency: 1.0,
        }
    }

    pub fn graph(&self) -> &LayoutGraph {
        &self.graph
    }

    pub fn settings(&self) -> &LayoutSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut LayoutSettings {
        &mut self.settings
    }

    pub fn positions(&self) -> &[f32] {
        &self.positions
    }

    /// Current adaptive global speed — a rough convergence signal (drops as
    /// the layout settles).
    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn run(&mut self, iterations: u32) {
        for _ in 0..iterations {
            self.step();
        }
    }

    /// One full ForceAtlas2 iteration: repulsion + gravity + attraction into
    /// the force buffer, then adaptive-speed integration.
    pub fn step(&mut self) {
        let n = self.graph.node_count();
        if n == 0 {
            return;
        }
        std::mem::swap(&mut self.forces, &mut self.prev_forces);
        self.forces.fill(0.0);

        self.apply_repulsion_and_gravity();
        self.apply_attraction();
        self.integrate();
    }

    fn apply_repulsion_and_gravity(&mut self) {
        let n = self.graph.node_count();
        let kr = self.settings.repulsion;
        let kg = self.settings.gravity;
        let strong = self.settings.strong_gravity;
        let theta = self.settings.theta;
        let positions = &self.positions;
        let masses = &self.masses;

        let tree = (self.settings.barnes_hut && n > BARNES_HUT_CUTOFF)
            .then(|| QuadTree::build(positions, masses));

        // Each node's force is computed independently from the shared
        // read-only positions/tree, writing only its own force slot — the
        // shape that lets the `parallel` feature fan the loop out with rayon
        // (and, later, wasm threads) without locks.
        let kernel = |i: usize, force: &mut [f32]| {
            let (x, y, m) = (positions[2 * i], positions[2 * i + 1], masses[i]);
            let (fx, fy) = match &tree {
                Some(tree) => tree.repulsion(i as u32, x, y, m, kr, theta),
                None => QuadTree::brute_force_repulsion(positions, masses, i, kr),
            };
            force[0] += fx;
            force[1] += fy;

            let dist = (x * x + y * y).sqrt();
            if dist > 1e-9 {
                // Toward the origin: constant magnitude kg*m, or
                // distance-proportional in strong mode.
                let factor = if strong { kg * m } else { kg * m / dist };
                force[0] -= x * factor;
                force[1] -= y * factor;
            }
        };

        #[cfg(feature = "parallel")]
        self.forces
            .par_chunks_exact_mut(2)
            .enumerate()
            .for_each(|(i, force)| kernel(i, force));

        #[cfg(not(feature = "parallel"))]
        self.forces
            .chunks_exact_mut(2)
            .enumerate()
            .for_each(|(i, force)| kernel(i, force));
    }

    fn apply_attraction(&mut self) {
        let lin_log = self.settings.lin_log;
        for &(source, target) in self.graph.edges() {
            let (s, t) = (source as usize * 2, target as usize * 2);
            let dx = self.positions[s] - self.positions[t];
            let dy = self.positions[s + 1] - self.positions[t + 1];
            let factor = if lin_log {
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > 1e-9 {
                    (1.0 + dist).ln() / dist
                } else {
                    0.0
                }
            } else {
                1.0 // linear attraction: force equals the delta vector
            };
            self.forces[s] -= dx * factor;
            self.forces[s + 1] -= dy * factor;
            self.forces[t] += dx * factor;
            self.forces[t + 1] += dy * factor;
        }
    }

    /// The FA2 adaptive displacement: per-node "swinging" (disagreement
    /// between successive forces) slows oscillating nodes down, while global
    /// speed rises as the layout stabilizes.
    fn integrate(&mut self) {
        let n = self.graph.node_count();
        let mut total_swinging = 0.0f32;
        let mut total_traction = 0.0f32;
        for i in 0..n {
            let (fx, fy) = (self.forces[2 * i], self.forces[2 * i + 1]);
            let (px, py) = (self.prev_forces[2 * i], self.prev_forces[2 * i + 1]);
            let swing = ((fx - px).powi(2) + (fy - py).powi(2)).sqrt();
            let traction = ((fx + px).powi(2) + (fy + py).powi(2)).sqrt() / 2.0;
            total_swinging += self.masses[i] * swing;
            total_traction += self.masses[i] * traction;
        }
        total_swinging = total_swinging.max(1e-9);
        total_traction = total_traction.max(1e-9);

        // Jitter tolerance scales with graph size (per the FA2 paper).
        let estimated = 0.05 * (n as f32).sqrt();
        let min_jt = estimated.sqrt();
        let max_jt = 10.0;
        let mut jt = self.settings.jitter_tolerance
            * (estimated * total_traction / (n as f32).powi(2)).clamp(min_jt, max_jt);

        if total_swinging / total_traction > 2.0 {
            if self.speed_efficiency > 0.05 {
                self.speed_efficiency *= 0.5;
            }
            jt = jt.max(self.settings.jitter_tolerance);
        }

        let target_speed = jt * self.speed_efficiency * total_traction / total_swinging;
        if total_swinging > jt * total_traction {
            if self.speed_efficiency > 0.05 {
                self.speed_efficiency *= 0.7;
            }
        } else if self.speed < 1000.0 {
            self.speed_efficiency *= 1.3;
        }
        // Never rise more than 50% per iteration: a speed spike scatters the
        // layout and it takes many iterations to recover.
        self.speed += (target_speed - self.speed).min(0.5 * self.speed);

        for i in 0..n {
            let (fx, fy) = (self.forces[2 * i], self.forces[2 * i + 1]);
            let (px, py) = (self.prev_forces[2 * i], self.prev_forces[2 * i + 1]);
            let swing = self.masses[i] * ((fx - px).powi(2) + (fy - py).powi(2)).sqrt();
            let factor = self.speed / (1.0 + (self.speed * swing).sqrt());
            self.positions[2 * i] += fx * factor;
            self.positions[2 * i + 1] += fy * factor;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance(positions: &[f32], a: usize, b: usize) -> f32 {
        let dx = positions[2 * a] - positions[2 * b];
        let dy = positions[2 * a + 1] - positions[2 * b + 1];
        (dx * dx + dy * dy).sqrt()
    }

    fn line_graph(n: usize) -> LayoutGraph {
        let edges = (0..n as u32 - 1).map(|i| (i, i + 1)).collect();
        LayoutGraph::new(n, edges).unwrap()
    }

    #[test]
    fn connected_nodes_end_up_closer_than_disconnected() {
        // Two 3-node triangles with no link between them.
        let edges = vec![(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3)];
        let graph = LayoutGraph::new(6, edges).unwrap();
        let mut layout = ForceAtlas2::new(graph, LayoutSettings::default());
        layout.run(300);
        let p = layout.positions();
        let intra = distance(p, 0, 1);
        let inter = distance(p, 0, 3);
        assert!(
            inter > 2.0 * intra,
            "clusters did not separate: intra {intra}, inter {inter}"
        );
    }

    #[test]
    fn layout_is_deterministic() {
        let mk = || {
            let mut layout = ForceAtlas2::new(line_graph(50), LayoutSettings::default());
            layout.run(100);
            layout.positions().to_vec()
        };
        assert_eq!(mk(), mk());
    }

    #[test]
    fn stays_finite_on_larger_graph() {
        // Big enough to cross the Barnes-Hut cutoff.
        let mut edges: Vec<(u32, u32)> = (0..499).map(|i| (i, i + 1)).collect();
        // A hub to create degree skew.
        edges.extend((1..100).map(|i| (0, i * 5)));
        let graph = LayoutGraph::new(500, edges).unwrap();
        let mut layout = ForceAtlas2::new(graph, LayoutSettings::default());
        layout.run(50);
        assert!(layout.positions().iter().all(|v| v.is_finite()));
        assert!(layout.speed().is_finite());
    }

    #[test]
    fn barnes_hut_tracks_exact_layout() {
        // Same graph, exact vs approximated repulsion: identical settings
        // should land in qualitatively similar layouts (compare diameters).
        let diameter = |barnes_hut: bool| {
            let settings = LayoutSettings {
                barnes_hut,
                ..Default::default()
            };
            let mut layout = ForceAtlas2::new(line_graph(200), settings);
            layout.run(200);
            let p = layout.positions();
            let mut max = 0.0f32;
            for i in 0..200 {
                for j in (i + 1)..200 {
                    max = max.max(distance(p, i, j));
                }
            }
            max
        };
        let (exact, approx) = (diameter(false), diameter(true));
        let ratio = approx / exact;
        assert!(
            (0.5..2.0).contains(&ratio),
            "diameters diverged: exact {exact}, barnes-hut {approx}"
        );
    }

    #[test]
    fn empty_and_single_node_graphs_are_fine() {
        let mut empty = ForceAtlas2::new(
            LayoutGraph::new(0, vec![]).unwrap(),
            LayoutSettings::default(),
        );
        empty.run(10);
        assert!(empty.positions().is_empty());

        let mut single = ForceAtlas2::new(
            LayoutGraph::new(1, vec![]).unwrap(),
            LayoutSettings::default(),
        );
        single.run(10);
        assert!(single.positions().iter().all(|v| v.is_finite()));
    }
}
