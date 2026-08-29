//! Coverage for the Azure Resource Graph → typed-model mapping. Unlike the
//! other clouds, Azure returns untyped rows and the provider builds each model
//! by navigating `properties` by hand — so the contract worth testing is that
//! navigation, exercised here with canned ARG rows via the pure `map_resources`
//! (no `az login`). `api/azure/client.rs` covers the fetch/pagination side.

use atlas_lib::cloud::azure::provider::map_resources;
use atlas_lib::cloud::definition::MicrosoftCollection;
use serde_json::{Value, json};

/// Pull the `Vec` out of the one collection variant, panicking otherwise.
macro_rules! variant {
    ($cols:expr, $v:ident) => {
        $cols
            .iter()
            .find_map(|c| match c {
                MicrosoftCollection::$v(items) => Some(items),
                _ => None,
            })
            .expect(concat!("collection ", stringify!($v), " present"))
    };
}

fn map(rows: Vec<Value>) -> Vec<MicrosoftCollection> {
    let (collections, report) = map_resources(rows);
    assert!(
        report.is_complete(),
        "unexpected mapping failure: {}",
        report.summary()
    );
    collections
}

/// ARG returns the whole tenant in one response, so a single drifted row must
/// not take the batch down with it: that would empty the Azure half of the
/// graph over one bad record. The skip is reported instead, which keeps the
/// differ from reading it as a deletion.
#[test]
fn a_malformed_row_is_skipped_without_discarding_the_batch() {
    let good = |id: &str, name: &str| {
        json!({
            "id": id,
            "name": name,
            "type": "microsoft.storage/storageaccounts",
            "location": "eastus"
        })
    };
    // `name` must be a string; an integer fails AzureResource deserialization.
    let malformed = json!({
        "id": "/subscriptions/s/.../storageAccounts/broken",
        "name": 12345,
        "type": "microsoft.storage/storageaccounts",
        "location": "eastus"
    });

    let (cols, report) = map_resources(vec![
        good("/subscriptions/s/.../storageAccounts/a", "sa-a"),
        malformed,
        good("/subscriptions/s/.../storageAccounts/b", "sa-b"),
    ]);

    let accounts = variant!(cols, AzureStorageAccounts);
    let names: Vec<&str> = accounts
        .iter()
        .filter_map(|a| a.name.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["sa-a", "sa-b"],
        "rows either side of the malformed one must survive"
    );

    assert!(
        !report.is_complete(),
        "a skipped row must be reported, or its absence looks like a deletion"
    );
    assert!(
        report.summary().contains("storageAccounts/broken"),
        "the report must name the offending row: {}",
        report.summary()
    );
}

/// A type we do not model is a deliberate filter, not a failure -- reporting it
/// would hold Azure's whole estate every single tick.
#[test]
fn an_unmodelled_resource_type_is_filtered_without_being_reported() {
    let (_cols, report) = map_resources(vec![json!({
        "id": "/subscriptions/s/.../redis/cache1",
        "name": "cache1",
        "type": "microsoft.cache/redis",
        "location": "eastus"
    })]);

    assert!(report.is_complete(), "got: {}", report.summary());
}

#[test]
fn vm_extracts_nic_ids_from_network_profile() {
    // Mixed-case `type` proves the provider's case-folding match.
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../virtualMachines/vm1",
        "name": "vm1",
        "type": "Microsoft.Compute/virtualMachines",
        "location": "eastus",
        "properties": { "networkProfile": { "networkInterfaces": [ { "id": "/subscriptions/s/.../nic1" } ] } }
    })]);

    let vms = variant!(cols, AzureVirtualMachines);
    assert_eq!(vms.len(), 1);
    assert_eq!(vms[0].name.as_deref(), Some("vm1"));
    assert_eq!(vms[0].location.as_deref(), Some("eastus"));
    assert_eq!(
        vms[0].network_interfaces,
        vec!["/subscriptions/s/.../nic1".to_string()]
    );
}

#[test]
fn vnet_extracts_inline_subnets_with_nsg() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../virtualNetworks/vnet1",
        "name": "vnet1",
        "type": "microsoft.network/virtualnetworks",
        "location": "eastus",
        "properties": { "subnets": [ {
            "id": "/subscriptions/s/.../subnets/sn1",
            "name": "sn1",
            "properties": { "networkSecurityGroup": { "id": "/subscriptions/s/.../nsg1" } }
        } ] }
    })]);

    let vnets = variant!(cols, AzureVirtualNetworks);
    assert_eq!(
        vnets[0].subnets,
        vec!["/subscriptions/s/.../subnets/sn1".to_string()]
    );

    // Subnets are hoisted out of the VNet properties into their own collection.
    let subnets = variant!(cols, AzureSubnets);
    assert_eq!(subnets[0].name.as_deref(), Some("sn1"));
    assert_eq!(
        subnets[0].vnet_id.as_deref(),
        Some("/subscriptions/s/.../virtualNetworks/vnet1")
    );
    assert_eq!(
        subnets[0].network_security_group_id.as_deref(),
        Some("/subscriptions/s/.../nsg1")
    );
}

#[test]
fn public_ip_extracts_ip_address() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../publicIPAddresses/pip1",
        "name": "pip1",
        "type": "microsoft.network/publicipaddresses",
        "location": "eastus",
        "properties": { "ipAddress": "20.1.2.3" }
    })]);
    let pips = variant!(cols, AzurePublicIpAddresses);
    assert_eq!(pips[0].ip_address.as_deref(), Some("20.1.2.3"));
}

#[test]
fn web_site_kind_splits_function_apps_from_app_services() {
    let cols = map(vec![
        json!({
            "id": "/f", "name": "fn-app", "type": "microsoft.web/sites", "location": "eastus",
            "kind": "functionapp,linux", "properties": {}
        }),
        json!({
            "id": "/w", "name": "web-app", "type": "microsoft.web/sites", "location": "eastus",
            "kind": "app,linux", "properties": {}
        }),
    ]);
    assert_eq!(variant!(cols, AzureFunctionApps).len(), 1);
    assert_eq!(
        variant!(cols, AzureFunctionApps)[0].name.as_deref(),
        Some("fn-app")
    );
    assert_eq!(
        variant!(cols, AzureAppServices)[0].name.as_deref(),
        Some("web-app")
    );
}

#[test]
fn unknown_resource_types_are_ignored() {
    let cols = map(vec![json!({
        "id": "/x", "name": "mystery", "type": "microsoft.unknown/thing", "location": "eastus",
        "properties": {}
    })]);
    assert!(variant!(cols, AzureVirtualMachines).is_empty());
    assert!(variant!(cols, AzureStorageAccounts).is_empty());
}
