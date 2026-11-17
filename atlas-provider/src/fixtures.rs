use crate::Settings;

pub const REGION: &str = "us-east-1";
pub const GCP_PROJECT: &str = "globex-prod";
pub const AZURE_SUBSCRIPTION: &str = "sub-globex";

pub fn settings() -> Settings {
    Settings {
        regions: vec![REGION.to_owned()],
        gcp_projects: Some(vec![GCP_PROJECT.to_owned()]),
        azure_subscriptions: Some(vec![AZURE_SUBSCRIPTION.to_owned()]),
        cloudflare: true,
        ..Default::default()
    }
}

pub const AZURE_VPN_ADDRESS: &str = "198.51.100.60";
pub const GCP_VPN_ADDRESS: &str = "198.51.100.70";
pub const AWS_VPN_ADDRESSES: [&str; 3] = ["192.0.2.101", "192.0.2.102", "192.0.2.103"];
pub const AZURE_SPOKE_RANGE: &str = "10.20.1.0/24";
pub const CLOUD_SQL_RANGE: &str = "10.20.0.0/24";
pub const DX_AMAZON_ADDRESS: &str = "169.254.60.1/30";
pub const DX_GOOGLE_ADDRESS: &str = "169.254.60.2/30";
pub const ER_PRIMARY_PREFIX: &str = "169.254.70.0/30";
pub const ER_SECONDARY_PREFIX: &str = "169.254.70.4/30";
pub const ER_GOOGLE_ADDRESS: &str = "169.254.70.1/30";
pub const ER_MICROSOFT_ADDRESS: &str = "169.254.70.2/30";
