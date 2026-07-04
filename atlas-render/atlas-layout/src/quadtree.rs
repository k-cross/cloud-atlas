//! Barnes-Hut quadtree over the interleaved position buffer.
//!
//! Cells live in one flat `Vec` (children are allocated as a contiguous
//! block of four), so building the tree each iteration is a handful of
//! reallocations rather than pointer chasing. Queries walk the tree with a
//! fixed-size stack — no per-query allocation, which keeps the repulsion
//! kernel safe to run from parallel workers.

/// Splitting stops at this depth; points that still collide are aggregated
/// into one leaf. Bounds the query stack and defuses coincident points.
const MAX_DEPTH: u32 = 24;

const NO_CELL: u32 = u32::MAX;
const NO_POINT: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Cell {
    /// Geometric center and half-extent of the square region.
    cx: f32,
    cy: f32,
    half: f32,
    /// Total mass inside. `com_*` accumulate mass-weighted sums during
    /// build and are normalized to the center of mass by `finalize`.
    mass: f32,
    com_x: f32,
    com_y: f32,
    /// Number of points inside this cell's region.
    count: u32,
    /// Index of the first of four contiguous children, or NO_CELL for a leaf.
    children: u32,
    /// For leaves: the resident point (first one, if several coincide).
    point: u32,
}

impl Cell {
    fn empty(cx: f32, cy: f32, half: f32) -> Self {
        Self {
            cx,
            cy,
            half,
            mass: 0.0,
            com_x: 0.0,
            com_y: 0.0,
            count: 0,
            children: NO_CELL,
            point: NO_POINT,
        }
    }
}

pub struct QuadTree {
    cells: Vec<Cell>,
}

impl Default for QuadTree {
    fn default() -> Self {
        Self::new()
    }
}

impl QuadTree {
    /// An empty tree that owns no allocation yet. Fill it with `rebuild`.
    pub fn new() -> Self {
        Self { cells: Vec::new() }
    }

    /// Rebuild in place for a new frame's positions, reusing the existing cell
    /// allocation. The layout builds a tree every physics step, so retaining
    /// the buffer across frames turns a per-step allocation of ~2N cells into
    /// a `clear` + refill that reuses capacity after the first iteration.
    pub fn rebuild(&mut self, positions: &[f32], masses: &[f32]) {
        let n = masses.len();
        debug_assert_eq!(positions.len(), 2 * n);
        self.cells.clear();
        if n == 0 {
            return;
        }

        // Bounding square, slightly inflated so no point sits exactly on the
        // border and child selection stays unambiguous.
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for i in 0..n {
            min_x = min_x.min(positions[2 * i]);
            max_x = max_x.max(positions[2 * i]);
            min_y = min_y.min(positions[2 * i + 1]);
            max_y = max_y.max(positions[2 * i + 1]);
        }
        let cx = (min_x + max_x) / 2.0;
        let cy = (min_y + max_y) / 2.0;
        let half = ((max_x - min_x).max(max_y - min_y) / 2.0) * 1.01 + 1e-3;

        self.cells.push(Cell::empty(cx, cy, half));
        for i in 0..n {
            self.insert(i as u32, positions[2 * i], positions[2 * i + 1], masses[i]);
        }
        self.finalize();
    }

    /// Convenience constructor: an owned tree built from a single frame.
    /// `positions` is interleaved `[x0, y0, x1, y1, ..]`; `masses` is per node.
    pub fn build(positions: &[f32], masses: &[f32]) -> Self {
        let mut tree = Self::new();
        tree.rebuild(positions, masses);
        tree
    }

    fn quadrant(cell: &Cell, x: f32, y: f32) -> usize {
        ((x >= cell.cx) as usize) | (((y >= cell.cy) as usize) << 1)
    }

    fn alloc_children(&mut self, cell_idx: usize) -> u32 {
        let Cell { cx, cy, half, .. } = self.cells[cell_idx];
        let quarter = half / 2.0;
        let first = self.cells.len() as u32;
        for dy in [-1.0f32, 1.0] {
            for dx in [-1.0f32, 1.0] {
                // Order must match `quadrant`: x bit 0, y bit 1.
                self.cells
                    .push(Cell::empty(cx + dx * quarter, cy + dy * quarter, quarter));
            }
        }
        self.cells[cell_idx].children = first;
        first
    }

    fn insert(&mut self, idx: u32, x: f32, y: f32, m: f32) {
        let mut cell_idx = 0usize;
        let mut depth = 0u32;
        loop {
            // Every cell on the descent path aggregates the new point.
            let cell = &mut self.cells[cell_idx];
            cell.mass += m;
            cell.com_x += x * m;
            cell.com_y += y * m;
            cell.count += 1;

            if cell.children != NO_CELL {
                cell_idx = (cell.children as usize) + Self::quadrant(cell, x, y);
                depth += 1;
                continue;
            }
            if cell.count == 1 {
                // Was empty; the point settles here.
                cell.point = idx;
                return;
            }
            if depth >= MAX_DEPTH {
                // Coincident (or near-coincident) points: aggregate in place.
                return;
            }
            // Occupied leaf: split, push the resident one level down, then
            // keep descending with the new point. The resident's aggregates
            // were added to this cell when it arrived; only the new child
            // needs them now.
            let resident = cell.point;
            // Before this insert the resident was alone in the leaf, so the
            // pre-update weighted sums were exactly its contribution —
            // recover its coordinates from them.
            let (res_x, res_y, res_m) = {
                let c = &self.cells[cell_idx];
                let m_res = c.mass - m;
                ((c.com_x - x * m) / m_res, (c.com_y - y * m) / m_res, m_res)
            };
            self.cells[cell_idx].point = NO_POINT;
            let first_child = self.alloc_children(cell_idx);
            let parent = self.cells[cell_idx];
            let child_idx = (first_child as usize) + Self::quadrant(&parent, res_x, res_y);
            let child = &mut self.cells[child_idx];
            child.mass = res_m;
            child.com_x = res_x * res_m;
            child.com_y = res_y * res_m;
            child.count = 1;
            child.point = resident;
            cell_idx = (first_child as usize) + Self::quadrant(&parent, x, y);
            depth += 1;
        }
    }

    /// Normalize weighted sums into centers of mass.
    fn finalize(&mut self) {
        for cell in &mut self.cells {
            if cell.count > 0 {
                cell.com_x /= cell.mass;
                cell.com_y /= cell.mass;
            }
        }
    }

    /// Accumulate the ForceAtlas2 repulsion acting on point `idx` at (x, y)
    /// with mass `m`: F = kr * m * m_other / d, directed apart. `theta` is
    /// the Barnes-Hut opening criterion (region size / distance); regions
    /// smaller than that are treated as a single super-node.
    pub fn repulsion(&self, idx: u32, x: f32, y: f32, m: f32, kr: f32, theta: f32) -> (f32, f32) {
        // An empty tree (0-node rebuild) has no root cell to seed the walk.
        if self.cells.is_empty() {
            return (0.0, 0.0);
        }
        let theta_sq = theta * theta;
        let (mut fx, mut fy) = (0.0f32, 0.0f32);
        // Worst case: 3 unexpanded siblings per level plus the current path.
        let mut stack = [0u32; (4 * MAX_DEPTH as usize) + 8];
        let mut top = 0usize;
        stack[top] = 0;
        top += 1;

        while top > 0 {
            top -= 1;
            let cell = &self.cells[stack[top] as usize];
            if cell.count == 0 {
                continue;
            }
            let dx = x - cell.com_x;
            let dy = y - cell.com_y;
            let d_sq = dx * dx + dy * dy;
            let size = cell.half * 2.0;

            let is_leaf = cell.children == NO_CELL;
            if is_leaf || (size * size) < theta_sq * d_sq {
                if is_leaf && cell.point == idx && cell.count == 1 {
                    continue; // the point itself
                }
                if d_sq > 1e-12 {
                    // F/d * (dx, dy)/d == F * (dx, dy) / d_sq
                    let factor = kr * m * cell.mass / d_sq;
                    fx += dx * factor;
                    fy += dy * factor;
                } else {
                    // Coincident with the aggregate (or another point):
                    // deterministic index-based nudge to break the tie.
                    let angle = (idx as f32) * 2.399_963_2; // golden angle
                    let f = kr * m * cell.mass;
                    fx += angle.cos() * f;
                    fy += angle.sin() * f;
                }
            } else {
                for c in 0..4 {
                    stack[top] = cell.children + c;
                    top += 1;
                }
            }
        }
        (fx, fy)
    }

    /// Exact pairwise repulsion on `idx`, same force law as `repulsion`.
    /// Used below the Barnes-Hut cutoff and as the reference in tests.
    pub fn brute_force_repulsion(
        positions: &[f32],
        masses: &[f32],
        idx: usize,
        kr: f32,
    ) -> (f32, f32) {
        let (x, y, m) = (positions[2 * idx], positions[2 * idx + 1], masses[idx]);
        let (mut fx, mut fy) = (0.0f32, 0.0f32);
        for j in 0..masses.len() {
            if j == idx {
                continue;
            }
            let dx = x - positions[2 * j];
            let dy = y - positions[2 * j + 1];
            let d_sq = dx * dx + dy * dy;
            if d_sq > 1e-12 {
                let factor = kr * m * masses[j] / d_sq;
                fx += dx * factor;
                fy += dy * factor;
            } else {
                let angle = (idx as f32) * 2.399_963_2;
                let f = kr * m * masses[j];
                fx += angle.cos() * f;
                fy += angle.sin() * f;
            }
        }
        (fx, fy)
    }

    #[cfg(test)]
    fn root(&self) -> &Cell {
        &self.cells[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random point cloud (splitmix64-derived).
    fn scatter(n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut positions = Vec::with_capacity(2 * n);
        let mut masses = Vec::with_capacity(n);
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z = z ^ (z >> 31);
            (z >> 40) as f32 / (1u64 << 24) as f32 // [0, 1)
        };
        for i in 0..n {
            positions.push(next() * 1000.0 - 500.0);
            positions.push(next() * 1000.0 - 500.0);
            masses.push(1.0 + (i % 7) as f32);
        }
        (positions, masses)
    }

    #[test]
    fn conserves_mass_and_count() {
        let (positions, masses) = scatter(500);
        let tree = QuadTree::build(&positions, &masses);
        let root = tree.root();
        assert_eq!(root.count, 500);
        let total: f32 = masses.iter().sum();
        assert!((root.mass - total).abs() < total * 1e-4);
    }

    #[test]
    fn approximates_brute_force() {
        let (positions, masses) = scatter(400);
        let tree = QuadTree::build(&positions, &masses);
        let mut total_rel_err = 0.0f32;
        for i in 0..masses.len() {
            let (ax, ay) = tree.repulsion(
                i as u32,
                positions[2 * i],
                positions[2 * i + 1],
                masses[i],
                1.0,
                0.5,
            );
            let (ex, ey) = QuadTree::brute_force_repulsion(&positions, &masses, i, 1.0);
            let err = ((ax - ex).powi(2) + (ay - ey).powi(2)).sqrt();
            let mag = (ex * ex + ey * ey).sqrt().max(1e-6);
            let rel = err / mag;
            total_rel_err += rel;
            // Individual nodes can see larger error when their exact force
            // nearly cancels out; the aggregate bound below is the real check.
            assert!(
                rel < 0.15,
                "node {i}: approx ({ax}, {ay}) vs exact ({ex}, {ey})"
            );
        }
        let mean_rel_err = total_rel_err / masses.len() as f32;
        assert!(
            mean_rel_err < 0.02,
            "mean relative error too high: {mean_rel_err}"
        );
    }

    #[test]
    fn theta_zero_is_exact() {
        let (positions, masses) = scatter(100);
        let tree = QuadTree::build(&positions, &masses);
        for i in 0..masses.len() {
            let (ax, ay) = tree.repulsion(
                i as u32,
                positions[2 * i],
                positions[2 * i + 1],
                masses[i],
                1.0,
                0.0,
            );
            let (ex, ey) = QuadTree::brute_force_repulsion(&positions, &masses, i, 1.0);
            assert!((ax - ex).abs() < 1e-2 && (ay - ey).abs() < 1e-2);
        }
    }

    #[test]
    fn empty_tree_repulsion_is_zero_not_a_panic() {
        // rebuild() over a 0-node frame leaves no root cell; querying it must
        // return zero force rather than indexing into an empty buffer.
        let mut tree = QuadTree::new();
        tree.rebuild(&[], &[]);
        assert_eq!(tree.repulsion(0, 0.0, 0.0, 1.0, 1.0, 0.5), (0.0, 0.0));
    }

    #[test]
    fn rebuild_reuses_buffer_and_matches_fresh_build() {
        // Reusing the allocation across frames must be indistinguishable from a
        // fresh build: same cells, so same repulsion, just no reallocation.
        let (p1, m1) = scatter(300);
        let (p2, m2) = scatter(300);
        let mut reused = QuadTree::build(&p1, &m1);
        reused.rebuild(&p2, &m2); // second frame reuses the buffer
        let fresh = QuadTree::build(&p2, &m2);
        for i in 0..m2.len() {
            let (rx, ry) = reused.repulsion(i as u32, p2[2 * i], p2[2 * i + 1], m2[i], 1.0, 0.5);
            let (fx, fy) = fresh.repulsion(i as u32, p2[2 * i], p2[2 * i + 1], m2[i], 1.0, 0.5);
            assert_eq!((rx, ry), (fx, fy), "node {i} diverged after reuse");
        }
    }

    #[test]
    fn survives_coincident_points() {
        // All points at the same spot: must not hang or produce NaN.
        let positions = vec![5.0f32; 20];
        let masses = vec![1.0f32; 10];
        let tree = QuadTree::build(&positions, &masses);
        let (fx, fy) = tree.repulsion(0, 5.0, 5.0, 1.0, 1.0, 0.5);
        assert!(fx.is_finite() && fy.is_finite());
        assert!(fx != 0.0 || fy != 0.0, "coincident points must still repel");
    }
}
