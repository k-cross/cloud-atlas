use crate::Settings;
use crate::cloud::definition::Provider;
use crate::cloud::google::instance;
use google_compute1::Compute;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use yup_oauth2::ApplicationSecret;

pub async fn build_gcp(
    _verbose: bool,
    opts: &Settings,
) -> Result<Provider, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    let secret: ApplicationSecret = Default::default();

    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .build()
    .await?;

    let https = HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_or_http()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build(https);

    let compute = Compute::new(client, auth);

    if let Some(projects) = &opts.gcp_projects {
        for p in projects {
            let (instances,) = tokio::try_join!(instance::collector::runner(p, &compute))?;

            services.push(instances);
        }
    }

    Ok(Provider::GCP(services))
}
