//! ForgeGraph HTTP client
//!
//! Communicates with the ForgeGraph tRPC API to list services and retrieve
//! connection credentials for managed database instances.

use serde::{Deserialize, Serialize};

/// Network transport descriptor for a ForgeGraph service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    pub kind: String,
    pub host: String,
    pub port: u16,
}

/// Summary of a ForgeGraph-managed service (no credentials).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeGraphService {
    pub app_slug: String,
    pub app_name: String,
    pub stage: String,
    pub kind: String,
    pub node_name: String,
    pub node_status: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub transports: Vec<Transport>,
}

/// Full connection details including credentials, returned by `services.connection`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeGraphConnection {
    pub app_slug: String,
    pub app_name: String,
    pub stage: String,
    pub kind: String,
    pub node_name: String,
    pub node_status: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub transports: Vec<Transport>,
    #[serde(default)]
    pub credentials: serde_json::Value,
}

/// Outer tRPC envelope — `{ result: { data: T } }`.
#[derive(Debug, Clone, Deserialize)]
pub struct TrpcResponse<T> {
    pub result: TrpcResult<T>,
}

/// Inner tRPC result — `{ data: T }`.
#[derive(Debug, Clone, Deserialize)]
pub struct TrpcResult<T> {
    pub data: T,
}

/// Fetch the list of all services from the ForgeGraph API.
pub async fn list_services(
    server: &str,
    token: &str,
) -> Result<Vec<ForgeGraphService>, String> {
    let url = format!("{}/api/trpc/services.list", server.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("ForgeGraph request failed: {}", e))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("ForgeGraph authentication failed. Check your API token.".to_string());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "ForgeGraph API error ({}): {}",
            status, body
        ));
    }

    let trpc: TrpcResponse<Vec<ForgeGraphService>> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse ForgeGraph response: {}", e))?;

    Ok(trpc.result.data)
}

/// Fetch connection credentials for a specific service.
pub async fn get_connection(
    server: &str,
    token: &str,
    app_slug: &str,
    stage: &str,
    kind: &str,
) -> Result<ForgeGraphConnection, String> {
    let input = serde_json::json!({
        "appSlug": app_slug,
        "stage": stage,
        "kind": kind,
    });
    let input_str = serde_json::to_string(&input)
        .map_err(|e| format!("Failed to serialize input: {}", e))?;
    let encoded = urlencoding::encode(&input_str);

    let url = format!(
        "{}/api/trpc/services.connection?input={}",
        server.trim_end_matches('/'),
        encoded
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("ForgeGraph request failed: {}", e))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("ForgeGraph authentication failed. Check your API token.".to_string());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "ForgeGraph API error ({}): {}",
            status, body
        ));
    }

    let trpc: TrpcResponse<ForgeGraphConnection> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse ForgeGraph response: {}", e))?;

    Ok(trpc.result.data)
}
