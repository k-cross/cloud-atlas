pub mod collector {
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_iam::types::{User, Policy, Group, Role};
    use aws_sdk_iam::{config::Region, Client, Error};
    use crate::cloud::definition::AmazonCollection;
    use std::collections::HashMap;

    async fn get_iam_info(client: &Client) -> Result<(Vec<User>, Vec<Role>, Vec<Group>, Vec<Policy>), Error> {
        let user_req = client
            .list_users()
            .send();

        let group_req = client
            .list_groups()
            .send();

        let policy_req = client
            .list_policies()
            .send();

        let role_req = client
            .list_policies()
            .send();

        let user_resp = user_req.await?;
        let role_resp = role_req.await?;
        let group_resp = group_req.await?;
        let policy_resp = policy_req.await?;

        let us = if let Some(users) = user_resp.users() {
            users.to_owned()
        } else { Vec::new() };

        let rs = if let Some(roles) = role_resp.roles() {
            roles.to_owned()
        } else { Vec::new() };

        let gs = if let Some(groups) = group_resp.groups() {
            groups.to_owned()
        } else { Vec::new() };

        let ps = if let Some(policies) = policy_resp.policies() {
            policies.to_owned()
        } else { Vec::new() };

        Ok((us, rs, gs, ps))
    }

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        match get_iam_info(&client).await {
            Ok((users, roles, groups, policies)) => {
                AmazonCollection::AmazonIAM {
                    groups: groups,
                    policies: policies,
                    roles: roles,
                    users: users,
                }
            }
            Err(e) => Err(e.into()),
        }
    }
}
