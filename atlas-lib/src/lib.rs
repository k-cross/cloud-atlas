pub mod api;
pub mod atlas;
pub mod cloud;

#[derive(Debug)]
pub struct Settings {
    /// The AWS Region.
    pub regions: Vec<String>,

    /// The GCP projects.
    pub gcp_projects: Option<Vec<String>>,

    /// The Azure subscriptions.
    pub azure_subscriptions: Option<Vec<String>>,

    /// Whether to include Cloudflare resources.
    pub cloudflare: bool,

    /// Include all mappings by default
    pub all: bool,

    /// Whether to display additional information.
    pub verbose: bool,

    /// Whether or not to exclude unknown graph entites by default or try to map them
    pub exclude_by_default: bool,
}
