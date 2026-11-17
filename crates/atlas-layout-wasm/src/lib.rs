use atlas_layout::{ForceAtlas2, LayoutGraph, LayoutSettings};
use js_sys::Float32Array;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct LayoutEngine {
    layout: ForceAtlas2,
}

#[wasm_bindgen]
impl LayoutEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(snapshot_json: &str) -> Result<LayoutEngine, JsError> {
        let graph =
            LayoutGraph::from_json(snapshot_json).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self {
            layout: ForceAtlas2::new(graph, LayoutSettings::default()),
        })
    }

    #[wasm_bindgen(js_name = fromEdgeList)]
    pub fn from_edge_list(node_count: u32, edges: &[u32]) -> Result<LayoutEngine, JsError> {
        if !edges.len().is_multiple_of(2) {
            return Err(JsError::new("edge list length must be even"));
        }
        let pairs = edges.chunks_exact(2).map(|e| (e[0], e[1])).collect();
        let graph = LayoutGraph::new(node_count as usize, pairs)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self {
            layout: ForceAtlas2::new(graph, LayoutSettings::default()),
        })
    }

    pub fn step(&mut self, iterations: u32) {
        self.layout.run(iterations);
    }

    #[wasm_bindgen(js_name = positionsView)]
    pub fn positions_view(&self) -> Float32Array {
        unsafe { Float32Array::view(self.layout.positions()) }
    }

    #[wasm_bindgen(js_name = positionsCopy)]
    pub fn positions_copy(&self) -> Float32Array {
        Float32Array::from(self.layout.positions())
    }

    pub fn speed(&self) -> f32 {
        self.layout.speed()
    }

    #[wasm_bindgen(js_name = nodeCount)]
    pub fn node_count(&self) -> u32 {
        self.layout.graph().node_count() as u32
    }

    #[wasm_bindgen(js_name = edgeCount)]
    pub fn edge_count(&self) -> u32 {
        self.layout.graph().edge_count() as u32
    }

    #[wasm_bindgen(js_name = setRepulsion)]
    pub fn set_repulsion(&mut self, value: f32) {
        self.layout.settings_mut().repulsion = value;
    }

    #[wasm_bindgen(js_name = setGravity)]
    pub fn set_gravity(&mut self, value: f32) {
        self.layout.settings_mut().gravity = value;
    }

    #[wasm_bindgen(js_name = setStrongGravity)]
    pub fn set_strong_gravity(&mut self, value: bool) {
        self.layout.settings_mut().strong_gravity = value;
    }

    #[wasm_bindgen(js_name = setLinLog)]
    pub fn set_lin_log(&mut self, value: bool) {
        self.layout.settings_mut().lin_log = value;
    }

    #[wasm_bindgen(js_name = setTheta)]
    pub fn set_theta(&mut self, value: f32) {
        self.layout.settings_mut().theta = value;
    }

    #[wasm_bindgen(js_name = setJitterTolerance)]
    pub fn set_jitter_tolerance(&mut self, value: f32) {
        self.layout.settings_mut().jitter_tolerance = value;
    }
}
