use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PublicRpcEndpoint {
    #[serde(default)]
    pub rpc_url: String,
    #[serde(default)]
    pub owner_address: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub best_height: u64,
    #[serde(default)]
    pub connected_peers: usize,
    #[serde(default)]
    pub remote_enabled: bool,
    #[serde(default)]
    pub last_seen: u64,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub last_error: String,
}

#[derive(Debug, Deserialize)]
struct RegistryListResponse {
    ok: bool,
    #[serde(default)]
    endpoints: Vec<PublicRpcEndpoint>,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize)]
struct RegistryPublishResponse {
    ok: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    error: String,
}

pub fn fetch_public_rpcs(registry_url: &str) -> Result<Vec<PublicRpcEndpoint>, String> {
    let status_url = registry_action_url(registry_url, "status")?;
    let response = ureq::get(&status_url)
        .call()
        .map_err(|err| format!("Unable to reach RPC registry: {err}"))?;
    let status_parsed: RegistryListResponse = response
        .into_json()
        .map_err(|err| format!("RPC registry returned invalid JSON: {err}"))?;
    if !status_parsed.ok {
        return Err(non_empty_or(
            status_parsed.error,
            "RPC registry reported a failure".to_string(),
        ));
    }

    let list_url = registry_action_url(registry_url, "list")?;
    let list_response = ureq::get(&list_url)
        .call()
        .map_err(|err| format!("Unable to reach RPC registry cache: {err}"))?;
    let list_parsed: RegistryListResponse = list_response
        .into_json()
        .map_err(|err| format!("RPC registry cache returned invalid JSON: {err}"))?;
    if !list_parsed.ok {
        return Err(non_empty_or(
            list_parsed.error,
            "RPC registry cache reported a failure".to_string(),
        ));
    }

    let mut merged = std::collections::BTreeMap::<String, PublicRpcEndpoint>::new();
    for endpoint in list_parsed.endpoints {
        merged.insert(endpoint.rpc_url.clone(), endpoint);
    }
    for endpoint in status_parsed.endpoints {
        merged.insert(endpoint.rpc_url.clone(), endpoint);
    }
    Ok(merged.into_values().collect())
}

pub fn publish_public_rpc(
    registry_url: &str,
    rpc_url: &str,
    owner_address: &str,
    best_height: u64,
    connected_peers: usize,
    remote_enabled: bool,
) -> Result<String, String> {
    let request_url = registry_action_url(registry_url, "publish")?;
    let response = ureq::post(&request_url)
        .set("Content-Type", "application/json")
        .send_json(json!({
            "rpc_url": rpc_url,
            "owner_address": owner_address,
            "source": "blindeye-gui",
            "best_height": best_height,
            "connected_peers": connected_peers,
            "remote_enabled": remote_enabled,
        }))
        .map_err(|err| format!("Unable to publish to RPC registry: {err}"))?;
    let parsed: RegistryPublishResponse = response
        .into_json()
        .map_err(|err| format!("RPC registry publish response was invalid JSON: {err}"))?;
    if !parsed.ok {
        return Err(non_empty_or(
            parsed.error,
            "RPC registry rejected the publish request".to_string(),
        ));
    }
    Ok(non_empty_or(
        parsed.message,
        "RPC published successfully".to_string(),
    ))
}

fn registry_action_url(registry_url: &str, action: &str) -> Result<String, String> {
    let trimmed = registry_url.trim();
    if trimmed.is_empty() {
        return Err("RPC registry URL is empty".to_string());
    }

    if trimmed.contains("action=") {
        return Ok(trimmed.to_string());
    }

    let separator = if trimmed.contains('?') { "&" } else { "?" };
    Ok(format!("{trimmed}{separator}action={action}"))
}

fn non_empty_or(value: String, fallback: String) -> String {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}
