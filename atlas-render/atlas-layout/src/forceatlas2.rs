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
    /// Per-node mass-weighted swing, cached by `integrate` so the totals pass
    /// and the displacement pass don't each recompute it.
    swings: Vec<f32>,
    masses: Vec<f32>,
    /// Warm-started nodes are pinned: they exert forces (repulsion mass,
    /// attraction anchors) but are never displaced, and are excluded from the
    /// adaptive-speed totals. This is what keeps an incremental update from
    /// re-flowing the whole layout — FA2 has no equilibrium, so re-running it
    /// over free nodes always drifts everything; pinning makes "only the
    /// change moves" a hard guarantee.
    fixed: Vec<bool>,
    free_count: usize,
    /// Reused across iterations so the Barnes-Hut tree isn't reallocated every
    /// step; empty until the first tree-based iteration.
    tree: QuadTree,
    speed: f32,
    speed_efficiency: f32,
}

impl ForceAtlas2 {
    pub fn new(graph: LayoutGraph, settings: LayoutSettings) -> Self {
        let n = graph.node_count();
        let positions = Self::initial_positions(&graph, n);

        // A node that arrived with warm-start coordinates is pinned; only the
        // engine-placed (fresh) nodes are free to move.
        let warm = graph.initial_positions();
        let fixed: Vec<bool> = if warm.len() == 2 * n {
            (0..n)
                .map(|i| warm[2 * i].is_finite() && warm[2 * i + 1].is_finite())
                .collect()
        } else {
            vec![false; n]
        };
        let free_count = fixed.iter().filter(|f| !**f).count();

        let masses = (0..n).map(|i| graph.mass(i)).collect();
        Self {
            graph,
            settings,
            positions,
            forces: vec![0.0; 2 * n],
            prev_forces: vec![0.0; 2 * n],
            swings: vec![0.0; n],
            masses,
            fixed,
            free_count,
            tree: QuadTree::new(),
            // Nothing can move ⇒ already converged; callers polling `speed()`
            // see "settled" immediately instead of spinning to an iteration cap.
            speed: if free_count == 0 && n > 0 { 0.0 } else { 1.0 },
            speed_efficiency: 1.0,
        }
    }

    /// A phyllotaxis-spiral point of the given index around `(cx, cy)`.
    /// Deterministic, evenly spread, and no two indices coincide.
    fn spiral_point(index: usize, cx: f32, cy: f32) -> (f32, f32) {
        let radius = 10.0 * (index as f32).sqrt();
        let angle = (index as f32) * 2.399_963_2; // golden angle in radians
        (cx + radius * angle.cos(), cy + radius * angle.sin())
    }

    /// Starting positions: warm-start coordinates from the graph where present,
    /// with any unplaced (`NaN`) nodes spiralled around the centroid of the
    /// placed ones so freshly-added nodes appear inside the existing cloud
    /// rather than flying in from the origin. With no warm-start data every
    /// node is spiralled from the origin (the deterministic cold start).
    fn initial_positions(graph: &LayoutGraph, n: usize) -> Vec<f32> {
        let warm = graph.initial_positions();
        if warm.len() != 2 * n {
            let mut positions = Vec::with_capacity(2 * n);
            for i in 0..n {
                let (x, y) = Self::spiral_point(i, 0.0, 0.0);
                positions.push(x);
                positions.push(y);
            }
            return positions;
        }

        // Centroid of the already-placed nodes anchors the newcomers.
        let (mut sx, mut sy, mut count) = (0.0f32, 0.0f32, 0usize);
        for i in 0..n {
            let (x, y) = (warm[2 * i], warm[2 * i + 1]);
            if x.is_finite() && y.is_finite() {
                sx += x;
                sy += y;
                count += 1;
            }
        }
        let (cx, cy) = if count > 0 {
            (sx / count as f32, sy / count as f32)
        } else {
            (0.0, 0.0)
        };

        let mut positions = Vec::with_capacity(2 * n);
        for i in 0..n {
            let (x, y) = (warm[2 * i], warm[2 * i + 1]);
            if x.is_finite() && y.is_finite() {
                positions.push(x);
                positions.push(y);
            } else {
                let (px, py) = Self::spiral_point(i, cx, cy);
                positions.push(px);
                positions.push(py);
            }
        }
        positions
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
        if n == 0 || self.free_count == 0 {
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
        let use_tree = self.settings.barnes_hut && n > BARNES_HUT_CUTOFF;

        // Disjoint field borrows: the tree (and read-only positions/masses) are
        // shared into the kernel while `forces` is written — in parallel under
        // the `parallel` feature. Borrowing `tree` off `self` also lets it
        // reuse its cell buffer across iterations instead of reallocating.
        let ForceAtlas2 {
            positions,
            masses,
            forces,
            tree,
            ..
        } = self;
        let positions: &[f32] = positions;
        let masses: &[f32] = masses;

        let tree = if use_tree {
            tree.rebuild(positions, masses);
            Some(&*tree)
        } else {
            None
        };

        // Each node's force is computed independently from the shared
        // read-only positions/tree, writing only its own force slot — the
        // shape that lets the `parallel` feature fan the loop out with rayon
        // (and, later, wasm threads) without locks.
        let kernel = |i: usize, force: &mut [f32]| {
            let (x, y, m) = (positions[2 * i], positions[2 * i + 1], masses[i]);
            let (fx, fy) = match tree {
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
        forces
            .par_chunks_exact_mut(2)
            .enumerate()
            .for_each(|(i, force)| kernel(i, force));

        #[cfg(not(feature = "parallel"))]
        forces
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
            // Pinned nodes don't move, so their (still-changing) forces must
            // not feed the speed controller — otherwise a mostly-pinned graph
            // never registers as converged.
            if self.fixed[i] {
                self.swings[i] = 0.0;
                continue;
            }
            let (fx, fy) = (self.forces[2 * i], self.forces[2 * i + 1]);
            let (px, py) = (self.prev_forces[2 * i], self.prev_forces[2 * i + 1]);
            // Mass-weighted swing is reused verbatim by the displacement pass
            // below, so cache it rather than recomputing the sqrt per node.
            let swing = self.masses[i] * ((fx - px).powi(2) + (fy - py).powi(2)).sqrt();
            let traction = self.masses[i] * ((fx + px).powi(2) + (fy + py).powi(2)).sqrt() / 2.0;
            self.swings[i] = swing;
            total_swinging += swing;
            total_traction += traction;
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
            if self.fixed[i] {
                continue;
            }
            let (fx, fy) = (self.forces[2 * i], self.forces[2 * i + 1]);
            let swing = self.swings[i];
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
    fn warm_start_seeds_provided_positions_and_places_newcomers_nearby() {
        use crate::graph::LayoutGraph;
        // Two placed nodes far from the origin, one freshly-added node (no x/y).
        let json = r#"{
            "version": 2,
            "nodes": [
                {"id": 0, "label": "a", "kind": "GenericIpAddress", "x": 100.0, "y": 100.0},
                {"id": 1, "label": "b", "kind": "GenericIpAddress", "x": 120.0, "y": 100.0},
                {"id": 2, "label": "c", "kind": "GenericIpAddress"}
            ],
            "edges": [{"source": 0, "target": 2, "kind": "RoutesTo"}]
        }"#;
        let graph = LayoutGraph::from_json(json).unwrap();
        let layout = ForceAtlas2::new(graph, LayoutSettings::default());
        let p = layout.positions();
        // Placed nodes start exactly where they were.
        assert_eq!((p[0], p[1]), (100.0, 100.0));
        assert_eq!((p[2], p[3]), (120.0, 100.0));
        // The newcomer is spiralled around the centroid (~110, 100), i.e. near
        // the existing cloud — not back at the origin.
        assert!(
            distance(p, 2, 0) < 60.0 && distance(p, 2, 1) < 60.0,
            "new node landed far from the warm cluster: {:?}",
            &p[4..6]
        );
    }

    #[test]
    fn warm_started_nodes_are_pinned_through_a_full_run() {
        use crate::graph::LayoutGraph;
        // A settled-looking cluster plus one newcomer wired into it. Running
        // the layout must move ONLY the newcomer: pinned nodes stay
        // bit-identical, so incremental updates can never re-flow the cloud.
        let json = r#"{
            "version": 2,
            "nodes": [
                {"id": 0, "label": "a", "kind": "GenericIpAddress", "x": 50.0, "y": 0.0},
                {"id": 1, "label": "b", "kind": "GenericIpAddress", "x": -50.0, "y": 0.0},
                {"id": 2, "label": "c", "kind": "GenericIpAddress", "x": 0.0, "y": 80.0},
                {"id": 3, "label": "new", "kind": "GenericHostname"}
            ],
            "edges": [
                {"source": 0, "target": 1, "kind": "RoutesTo"},
                {"source": 0, "target": 3, "kind": "ResolvesTo"}
            ]
        }"#;
        let graph = LayoutGraph::from_json(json).unwrap();
        let mut layout = ForceAtlas2::new(graph, LayoutSettings::default());
        let newcomer_start = (layout.positions()[6], layout.positions()[7]);
        layout.run(300);
        let p = layout.positions();
        assert_eq!(
            &p[0..6],
            &[50.0, 0.0, -50.0, 0.0, 0.0, 80.0],
            "pinned nodes moved"
        );
        assert!(
            (p[6], p[7]) != newcomer_start,
            "the free newcomer should have been laid out"
        );
        assert!(p.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn fully_pinned_graph_reports_settled_immediately() {
        use crate::graph::LayoutGraph;
        // A removal-only update warm-starts every surviving node — nothing can
        // move, so the engine must present as converged without iterating.
        let json = r#"{
            "version": 2,
            "nodes": [
                {"id": 0, "label": "a", "kind": "GenericIpAddress", "x": 1.0, "y": 2.0},
                {"id": 1, "label": "b", "kind": "GenericIpAddress", "x": 3.0, "y": 4.0}
            ],
            "edges": [{"source": 0, "target": 1, "kind": "RoutesTo"}]
        }"#;
        let graph = LayoutGraph::from_json(json).unwrap();
        let mut layout = ForceAtlas2::new(graph, LayoutSettings::default());
        assert_eq!(layout.speed(), 0.0);
        layout.run(50);
        assert_eq!(layout.positions(), &[1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn cold_start_without_positions_still_spirals_from_origin() {
        use crate::graph::LayoutGraph;
        // No x/y anywhere → the first node sits at the spiral origin.
        let json = r#"{
            "version": 2,
            "nodes": [
                {"id": 0, "label": "a", "kind": "GenericIpAddress"},
                {"id": 1, "label": "b", "kind": "GenericIpAddress"}
            ],
            "edges": []
        }"#;
        let graph = LayoutGraph::from_json(json).unwrap();
        let layout = ForceAtlas2::new(graph, LayoutSettings::default());
        assert_eq!((layout.positions()[0], layout.positions()[1]), (0.0, 0.0));
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
