pub mod collector {
    use crate::cloud::definition::GoogleCollection;
    use google_compute1::Compute;

    // We can just use the generic type from Compute instead of explicitly typing it if we want,
    // but C must implement the right trait. In `google-apis-common`, it usually just binds to `hyper_util::client::legacy::connect::Connect`.
    pub async fn runner<C>(
        project: &str,
        client: &Compute<C>,
    ) -> Result<GoogleCollection, Box<dyn std::error::Error>>
    where
        C: Clone + Send + Sync + 'static + hyper_util::client::legacy::connect::Connect,
    {
        let mut all_instances = Vec::new();

        // In GCP, instances are per zone, but we can use aggregatedList to get them across all zones
        let mut call = client.instances().aggregated_list(project);

        loop {
            let (_resp, result) = call.doit().await?;

            if let Some(items) = result.items {
                for (_, instances_scoped_list) in items {
                    if let Some(instances) = instances_scoped_list.instances {
                        for inst in instances {
                            all_instances.push(inst);
                        }
                    }
                }
            }

            if let Some(token) = result.next_page_token {
                call = client
                    .instances()
                    .aggregated_list(project)
                    .page_token(&token);
            } else {
                break;
            }
        }

        Ok(GoogleCollection::GoogleInstances(all_instances))
    }
}
