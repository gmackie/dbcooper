use crate::commands::cloudflare::r2_s3_config_from_settings;
use crate::db::models::{Connection, Setting};
use crate::s3::{
    open_downloaded_path, resolve_download_tmp_dir, unique_download_path, S3BucketInfo, S3Client,
    S3Config, S3ListResult, S3Object, S3ObjectPreview,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Source {
    pub connection_uuid: Option<String>,
    #[serde(default)]
    pub cloudflare: bool,
    pub bucket: Option<String>,
}

fn parse_s3_extra(extra: &Option<String>) -> serde_json::Value {
    extra
        .as_ref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn extra_bool(extra: &serde_json::Value, key: &str, default: bool) -> bool {
    extra.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn extra_str(extra: &serde_json::Value, key: &str) -> Option<String> {
    extra
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn config_from_connection(conn: &Connection) -> S3Config {
    let extra = parse_s3_extra(&conn.extra);
    let endpoint = extra_str(&extra, "endpoint").unwrap_or_else(|| conn.host.clone());
    let region = extra_str(&extra, "region").unwrap_or_else(|| "us-east-1".to_string());
    let path_style = extra_bool(&extra, "path_style", !endpoint.is_empty());
    let prefix = extra_str(&extra, "prefix");
    let bucket = if conn.database.is_empty() {
        None
    } else {
        Some(conn.database.clone())
    };
    S3Config {
        endpoint,
        region,
        access_key: conn.username.clone(),
        secret_key: conn.password.clone(),
        path_style,
        bucket,
        prefix,
    }
}

async fn resolve_client(
    pool: &SqlitePool,
    source: &S3Source,
) -> Result<(S3Client, Option<String>), String> {
    if source.cloudflare {
        let mut config = r2_s3_config_from_settings(pool).await?;
        if let Some(bucket) = &source.bucket {
            config.bucket = Some(bucket.clone());
        }
        let bucket = config.bucket.clone().or_else(|| source.bucket.clone());
        return Ok((S3Client::new(config), bucket));
    }
    let uuid = source
        .connection_uuid
        .as_ref()
        .ok_or_else(|| "S3 connection uuid is required".to_string())?;
    let conn: Connection = sqlx::query_as("SELECT * FROM connections WHERE uuid = ?")
        .bind(uuid)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to get S3 connection: {e}"))?;
    let mut config = config_from_connection(&conn);
    if let Some(bucket) = &source.bucket {
        config.bucket = Some(bucket.clone());
    }
    let bucket = source.bucket.clone().or(config.bucket.clone());
    Ok((S3Client::new(config), bucket))
}

fn require_bucket(bucket: Option<String>) -> Result<String, String> {
    bucket.filter(|b| !b.is_empty()).ok_or_else(|| {
        "Bucket is required. Select a bucket or set one on the connection.".to_string()
    })
}

#[tauri::command]
pub async fn s3_test_connection(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
) -> Result<String, String> {
    let (client, _) = resolve_client(sqlite_pool.inner(), &source).await?;
    client.test().await?;
    Ok("Connection successful!".to_string())
}

#[tauri::command]
pub async fn s3_list_buckets(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
) -> Result<Vec<S3BucketInfo>, String> {
    if source.cloudflare {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM cloudflare_resources WHERE kind = 'r2' ORDER BY name",
        )
        .fetch_all(sqlite_pool.inner())
        .await
        .map_err(|e| e.to_string())?;
        if !rows.is_empty() {
            return Ok(rows
                .into_iter()
                .map(|(name,)| S3BucketInfo {
                    name,
                    creation_date: None,
                })
                .collect());
        }
    }
    let (client, _) = resolve_client(sqlite_pool.inner(), &source).await?;
    client.list_buckets().await
}

#[tauri::command]
pub async fn s3_list_objects(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    prefix: Option<String>,
    continuation_token: Option<String>,
) -> Result<S3ListResult, String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client
        .list_objects(
            &bucket,
            prefix.as_deref(),
            Some("/"),
            continuation_token.as_deref(),
        )
        .await
}

#[tauri::command]
pub async fn s3_head_object(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    key: String,
) -> Result<S3Object, String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client.head_object(&bucket, &key).await
}

#[tauri::command]
pub async fn s3_preview_object(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    key: String,
) -> Result<S3ObjectPreview, String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client.preview_object(&bucket, &key).await
}

async fn configured_download_tmp_dir(pool: &SqlitePool) -> Result<PathBuf, String> {
    let row: Option<Setting> =
        sqlx::query_as("SELECT key, value FROM settings WHERE key = ?")
            .bind("download_tmp_dir")
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(resolve_download_tmp_dir(
        row.map(|s| s.value).as_deref(),
    ))
}

#[tauri::command]
pub async fn s3_download_tmp_dir(sqlite_pool: State<'_, SqlitePool>) -> Result<String, String> {
    Ok(configured_download_tmp_dir(sqlite_pool.inner())
        .await?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub async fn s3_open_object(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    key: String,
) -> Result<String, String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    let dir = configured_download_tmp_dir(sqlite_pool.inner()).await?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = unique_download_path(&dir, &key);
    client.download_object(&bucket, &key, &dest).await?;
    open_downloaded_path(&dest)?;
    Ok(dest.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn s3_download_object(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    key: String,
    dest_path: String,
) -> Result<(), String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client
        .download_object(&bucket, &key, &PathBuf::from(dest_path))
        .await
}

#[tauri::command]
pub async fn s3_upload_object(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    key: String,
    file_path: String,
) -> Result<(), String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client
        .upload_file(&bucket, &key, &PathBuf::from(file_path))
        .await
}

#[tauri::command]
pub async fn s3_delete_objects(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    keys: Vec<String>,
) -> Result<(), String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client.delete_objects(&bucket, &keys).await
}

#[tauri::command]
pub async fn s3_copy_object(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    from_key: String,
    to_key: String,
) -> Result<(), String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client.copy_object(&bucket, &from_key, &to_key).await
}

#[tauri::command]
pub async fn s3_create_folder(
    sqlite_pool: State<'_, SqlitePool>,
    source: S3Source,
    prefix: String,
) -> Result<(), String> {
    let (client, bucket) = resolve_client(sqlite_pool.inner(), &source).await?;
    let bucket = require_bucket(bucket)?;
    client.create_folder(&bucket, &prefix).await
}

#[tauri::command]
pub async fn s3_test_form(
    endpoint: String,
    region: String,
    access_key: String,
    secret_key: String,
    bucket: Option<String>,
    path_style: Option<bool>,
) -> Result<String, String> {
    let path_style = path_style.unwrap_or(!endpoint.is_empty());
    let client = S3Client::new(S3Config {
        endpoint,
        region: if region.is_empty() {
            "us-east-1".to_string()
        } else {
            region
        },
        access_key,
        secret_key,
        path_style,
        bucket,
        prefix: None,
    });
    client.test().await?;
    Ok("Connection successful!".to_string())
}
