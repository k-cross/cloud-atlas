pub mod atlas;
pub mod cloud;

#[derive(Debug)]
pub struct Settings {
    /// The AWS Region.
    pub regions: Vec<String>,

    /// Include all mappings by default
    pub all: bool,

    /// Whether to display additional information.
    pub verbose: bool,
}
