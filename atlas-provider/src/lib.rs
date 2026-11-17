pub mod collector;
pub mod projector;

use atlas_graph::collection::CollectionReport;

#[derive(Debug, Default)]
pub struct Settings {
    pub regions: Vec<String>,
    pub gcp_projects: Option<Vec<String>>,
    pub azure_subscriptions: Option<Vec<String>>,
    pub cloudflare: bool,
    pub verbose: bool,
    pub exclude_by_default: bool,
}

#[derive(Debug)]
pub struct ProviderScan<T> {
    pub collections: T,
    pub report: CollectionReport,
}

impl<T> ProviderScan<T> {
    pub fn map<U>(self, wrap: impl FnOnce(T) -> U) -> ProviderScan<U> {
        ProviderScan {
            collections: wrap(self.collections),
            report: self.report,
        }
    }
}
