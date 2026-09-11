pub mod api;
pub mod atlas;
pub mod cloud;
pub mod fixtures;

#[derive(Debug, Default)]
pub struct Settings {
    pub regions: Vec<String>,
    pub gcp_projects: Option<Vec<String>>,
    pub azure_subscriptions: Option<Vec<String>>,
    pub cloudflare: bool,
    pub verbose: bool,
    pub exclude_by_default: bool,
}
