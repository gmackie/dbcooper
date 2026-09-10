use crate::cloudflare::{CloudflareClient, D1Database, R2Bucket, R2S3Credentials};
use crate::database::pool_manager::{ConnectionConfig, ConnectionStatus, PoolManager};
use crate::db::models::Setting;
use crate::s3::S3Config;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tauri::State;

use super::pool::ConnectionStatusResponse;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CachedCloudflareResource {
    pub id: i64,
    pub kind: String,
    pub resource_id: String,
    pub name: String,
    pub account_id: String,
    pub extra: Option<String>,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareTestResult {
    pub account_id: String,
    pub accounts: Vec<CloudflareAccountView>,
    pub d1_count: usize,
    pub r2_count: usize,
    pub d1_error: Option<String>,
    pub r2_error: Option<String>,
    pub token_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareAccountView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareSyncResult {
    pub resources: Vec<CachedCloudflareResource>,
    pub d1_error: Option<String>,
    pub r2_error: Option<String>,
}

async fn setting_value(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    let row: Option<Setting> = sqlx::query_as("SELECT key, value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(|s| s.value).filter(|v| !v.is_empty()))
}

pub async fn read_cloudflare_settings(
    pool: &SqlitePool,
) -> Result<(String, String, Option<String>, Option<String>), String> {
    let token = setting_value(pool, "cloudflare_api_token")
        .await?
        .map(|t| crate::cloudflare::normalize_api_token(&t))
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "Cloudflare API token not configured".to_string())?;
    let account_id = setting_value(pool, "cloudflare_account_id")
        .await?
        .unwrap_or_default();
    let r2_key = setting_value(pool, "cloudflare_r2_access_key").await?;
    let r2_secret = setting_value(pool, "cloudflare_r2_secret_key").await?;
    Ok((token, account_id, r2_key, r2_secret))
}

pub fn d1_pool_key(account_id: &str, database_id: &str) -> String {
    format!("cf:d1:{account_id}:{database_id}")
}

pub fn r2_pool_key(account_id: &str, bucket: &str) -> String {
    format!("cf:r2:{account_id}:{bucket}")
}

pub async fn r2_s3_config_from_settings(pool: &SqlitePool) -> Result<S3Config, String> {
    let (token, account_id, r2_key, r2_secret) = read_cloudflare_settings(pool).await?;
    if account_id.is_empty() {
        return Err("Cloudflare account ID not configured".to_string());
    }
    let (access_key, secret_key) = if let (Some(key), Some(secret)) = (r2_key, r2_secret) {
        (key, secret)
    } else {
        let client = CloudflareClient::new(&token);
        let auth = client.authenticate(Some(&account_id)).await?;
        let token_id = auth.token_id.ok_or_else(|| {
            "Could not resolve Cloudflare token id for R2 key derivation. Set R2 Access Key and Secret in Settings.".to_string()
        })?;
        let derived: R2S3Credentials = client.derive_r2_credentials(&token_id);
        (derived.access_key_id, derived.secret_access_key)
    };
    Ok(S3Config {
        endpoint: format!("https://{account_id}.r2.cloudflarestorage.com"),
        region: "auto".to_string(),
        access_key,
        secret_key,
        path_style: true,
        bucket: None,
        prefix: None,
    })
}

#[tauri::command]
pub async fn cloudflare_is_configured(sqlite_pool: State<'_, SqlitePool>) -> Result<bool, String> {
    Ok(setting_value(sqlite_pool.inner(), "cloudflare_api_token")
        .await?
        .is_some())
}

#[tauri::command]
pub async fn cloudflare_test(
    sqlite_pool: State<'_, SqlitePool>,
) -> Result<CloudflareTestResult, String> {
    let (token, mut account_id, _, _) = read_cloudflare_settings(sqlite_pool.inner()).await?;
    let client = CloudflareClient::new(&token);
    let auth = client
        .authenticate((!account_id.is_empty()).then_some(account_id.as_str()))
        .await?;
    let accounts = auth.accounts;
    if account_id.is_empty() {
        if accounts.len() == 1 {
            account_id = accounts[0].id.clone();
            sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
                .bind("cloudflare_account_id")
                .bind(&account_id)
                .execute(sqlite_pool.inner())
                .await
                .map_err(|e| e.to_string())?;
        } else if accounts.is_empty() {
            return Err("Token is valid but no accounts were returned".to_string());
        }
    }

    let mut d1_error = None;
    let mut r2_error = None;
    let d1_count = if account_id.is_empty() {
        0
    } else {
        match client.list_d1_databases(&account_id).await {
            Ok(list) => list.len(),
            Err(e) => {
                d1_error = Some(e);
                0
            }
        }
    };
    let r2_count = if account_id.is_empty() {
        0
    } else {
        match client.list_r2_buckets(&account_id).await {
            Ok(list) => list.len(),
            Err(e) => {
                r2_error = Some(e);
                0
            }
        }
    };

    Ok(CloudflareTestResult {
        account_id,
        accounts: accounts
            .into_iter()
            .map(|a| CloudflareAccountView {
                id: a.id,
                name: a.name,
            })
            .collect(),
        d1_count,
        r2_count,
        d1_error,
        r2_error,
        token_id: auth.token_id,
    })
}

#[tauri::command]
pub async fn cloudflare_sync(
    sqlite_pool: State<'_, SqlitePool>,
) -> Result<CloudflareSyncResult, String> {
    let (token, account_id, _, _) = read_cloudflare_settings(sqlite_pool.inner()).await?;
    if account_id.is_empty() {
        return Err("Cloudflare account ID not configured. Use Test Connection first.".to_string());
    }
    let client = CloudflareClient::new(&token);

    let mut d1_error = None;
    let mut r2_error = None;
    let d1_list: Vec<D1Database> = match client.list_d1_databases(&account_id).await {
        Ok(list) => list,
        Err(e) => {
            d1_error = Some(e);
            Vec::new()
        }
    };
    let r2_list: Vec<R2Bucket> = match client.list_r2_buckets(&account_id).await {
        Ok(list) => list,
        Err(e) => {
            r2_error = Some(e);
            Vec::new()
        }
    };

    let mut tx = sqlite_pool
        .inner()
        .begin()
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM cloudflare_resources")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for db in &d1_list {
        let extra = serde_json::json!({
            "file_size": db.file_size,
            "num_tables": db.num_tables,
            "created_at": db.created_at,
        })
        .to_string();
        sqlx::query(
            "INSERT INTO cloudflare_resources (kind, resource_id, name, account_id, extra)
             VALUES ('d1', ?, ?, ?, ?)",
        )
        .bind(&db.uuid)
        .bind(&db.name)
        .bind(&account_id)
        .bind(extra)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    for bucket in &r2_list {
        let extra = serde_json::json!({
            "creation_date": bucket.creation_date,
            "jurisdiction": bucket.jurisdiction,
        })
        .to_string();
        sqlx::query(
            "INSERT INTO cloudflare_resources (kind, resource_id, name, account_id, extra)
             VALUES ('r2', ?, ?, ?, ?)",
        )
        .bind(&bucket.name)
        .bind(&bucket.name)
        .bind(&account_id)
        .bind(extra)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;

    let resources = cloudflare_list_cached_inner(sqlite_pool.inner()).await?;
    Ok(CloudflareSyncResult {
        resources,
        d1_error,
        r2_error,
    })
}

async fn cloudflare_list_cached_inner(
    pool: &SqlitePool,
) -> Result<Vec<CachedCloudflareResource>, String> {
    sqlx::query_as(
        "SELECT id, kind, resource_id, name, account_id, extra, synced_at FROM cloudflare_resources ORDER BY kind, name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cloudflare_list_cached(
    sqlite_pool: State<'_, SqlitePool>,
) -> Result<Vec<CachedCloudflareResource>, String> {
    cloudflare_list_cached_inner(sqlite_pool.inner()).await
}

#[tauri::command]
pub async fn cloudflare_connect(
    sqlite_pool: State<'_, SqlitePool>,
    pool_manager: State<'_, PoolManager>,
    kind: String,
    resource_id: String,
) -> Result<ConnectionStatusResponse, String> {
    let (token, account_id, _, _) = read_cloudflare_settings(sqlite_pool.inner()).await?;
    if account_id.is_empty() {
        return Err("Cloudflare account ID not configured".to_string());
    }
    match kind.as_str() {
        "d1" => {
            let pool_key = d1_pool_key(&account_id, &resource_id);
            let config = ConnectionConfig {
                db_type: "d1".to_string(),
                host: Some(account_id),
                port: Some(0),
                database: Some(resource_id),
                username: None,
                password: Some(token),
                ssl: Some(true),
                file_path: None,
                ssh_enabled: false,
                ssh_host: None,
                ssh_port: None,
                ssh_user: None,
                ssh_password: None,
                ssh_key_path: None,
                extra: None,
            };
            match pool_manager.connect(&pool_key, config).await {
                Ok(_) => Ok(ConnectionStatusResponse {
                    status: ConnectionStatus::Connected,
                    error: None,
                }),
                Err(e) => Ok(ConnectionStatusResponse {
                    status: ConnectionStatus::Disconnected,
                    error: Some(e),
                }),
            }
        }
        "r2" => Ok(ConnectionStatusResponse {
            status: ConnectionStatus::Connected,
            error: None,
        }),
        other => Err(format!("Unsupported Cloudflare resource kind: {other}")),
    }
}

#[tauri::command]
pub async fn cloudflare_disconnect(
    sqlite_pool: State<'_, SqlitePool>,
    pool_manager: State<'_, PoolManager>,
    kind: String,
    resource_id: String,
) -> Result<(), String> {
    let account_id = setting_value(sqlite_pool.inner(), "cloudflare_account_id")
        .await?
        .unwrap_or_default();
    if kind == "d1" {
        pool_manager
            .disconnect(&d1_pool_key(&account_id, &resource_id))
            .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn cloudflare_pool_key(
    sqlite_pool: State<'_, SqlitePool>,
    kind: String,
    resource_id: String,
) -> Result<String, String> {
    let account_id = setting_value(sqlite_pool.inner(), "cloudflare_account_id")
        .await?
        .unwrap_or_default();
    Ok(match kind.as_str() {
        "d1" => d1_pool_key(&account_id, &resource_id),
        "r2" => r2_pool_key(&account_id, &resource_id),
        other => format!("cf:{other}:{account_id}:{resource_id}"),
    })
}
