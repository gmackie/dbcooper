//! S3-compatible client used for generic S3 connections and Cloudflare R2.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusty_s3::actions::{
    CompleteMultipartUpload, CreateMultipartUpload, DeleteObject, GetObject, HeadObject,
    ListObjectsV2, PutObject, UploadPart,
};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

const SIGN_SECS: Duration = Duration::from_secs(3600);
const MULTIPART_THRESHOLD: usize = 8 * 1024 * 1024;
const PART_SIZE: usize = 8 * 1024 * 1024;
const PREVIEW_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub path_style: bool,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Object {
    pub key: String,
    pub size: i64,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
    pub storage_class: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Prefix {
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ListResult {
    pub prefixes: Vec<S3Prefix>,
    pub objects: Vec<S3Object>,
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketInfo {
    pub name: String,
    pub creation_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectPreview {
    pub key: String,
    pub content_type: Option<String>,
    pub size: i64,
    pub last_modified: Option<String>,
    pub is_text: bool,
    pub is_image: bool,
    pub text: Option<String>,
    pub data_base64: Option<String>,
    pub truncated: bool,
}

pub struct S3Client {
    config: S3Config,
    http: reqwest::Client,
}

impl S3Client {
    pub fn new(config: S3Config) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    fn credentials(&self) -> Credentials {
        Credentials::new(&self.config.access_key, &self.config.secret_key)
    }

    fn endpoint_url(&self) -> Result<url::Url, String> {
        let endpoint = if self.config.endpoint.is_empty() {
            format!("https://s3.{}.amazonaws.com", self.config.region)
        } else if self.config.endpoint.starts_with("http://")
            || self.config.endpoint.starts_with("https://")
        {
            self.config.endpoint.clone()
        } else {
            format!("https://{}", self.config.endpoint)
        };
        endpoint.parse().map_err(|e| format!("Invalid S3 endpoint: {e}"))
    }

    fn bucket(&self, name: &str) -> Result<Bucket, String> {
        let url_style = if self.config.path_style {
            UrlStyle::Path
        } else {
            UrlStyle::VirtualHost
        };
        let region = if self.config.region.is_empty() {
            "us-east-1".to_string()
        } else {
            self.config.region.clone()
        };
        Bucket::new(
            self.endpoint_url()?,
            url_style,
            name.to_string(),
            region,
        )
        .map_err(|e| format!("Invalid S3 bucket config: {e}"))
    }

    pub async fn test(&self) -> Result<(), String> {
        if let Some(bucket) = &self.config.bucket {
            let _ = self.list_objects(bucket, None, None, None).await?;
            Ok(())
        } else {
            let _ = self.list_buckets().await?;
            Ok(())
        }
    }

    pub async fn list_buckets(&self) -> Result<Vec<S3BucketInfo>, String> {
        if let Some(name) = &self.config.bucket {
            return Ok(vec![S3BucketInfo {
                name: name.clone(),
                creation_date: None,
            }]);
        }
        Err(
            "Set a bucket on the connection. Listing all buckets is not supported for this endpoint."
                .to_string(),
        )
    }

    pub async fn list_objects(
        &self,
        bucket_name: &str,
        prefix: Option<&str>,
        delimiter: Option<&str>,
        continuation_token: Option<&str>,
    ) -> Result<S3ListResult, String> {
        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        let mut action = ListObjectsV2::new(&bucket, Some(&creds));
        // rusty-s3 always sets encoding-type=url, which percent-encodes
        // CommonPrefixes (`artifacts%2F`). Prefer raw keys so the UI can
        // slice folder names against the current prefix.
        action.query_mut().remove("encoding-type");
        if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
            action.with_prefix(prefix.to_string());
        }
        if let Some(delimiter) = delimiter {
            action.query_mut().insert("delimiter", delimiter.to_string());
        }
        if let Some(token) = continuation_token.filter(|t| !t.is_empty()) {
            action.with_continuation_token(token.to_string());
        }
        let signed = action.sign(SIGN_SECS);
        let response = self
            .http
            .get(signed)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        let body = response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format_s3_error(status.as_u16(), &body));
        }
        parse_list_objects(&body)
    }

    pub async fn head_object(&self, bucket_name: &str, key: &str) -> Result<S3Object, String> {
        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        let action = HeadObject::new(&bucket, Some(&creds), key);
        let signed = action.sign(SIGN_SECS);
        let response = self
            .http
            .head(signed)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HeadObject failed ({})", response.status()));
        }
        let size = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        Ok(S3Object {
            key: key.to_string(),
            size,
            last_modified,
            etag,
            storage_class: None,
        })
    }

    pub async fn preview_object(
        &self,
        bucket_name: &str,
        key: &str,
    ) -> Result<S3ObjectPreview, String> {
        let meta = self.head_object(bucket_name, key).await.ok();
        let size = meta.as_ref().map(|m| m.size).unwrap_or(0);
        let content_type = guess_content_type(key);
        let is_image = content_type
            .as_deref()
            .map(|ct| ct.starts_with("image/"))
            .unwrap_or(false);
        let is_text = content_type
            .as_deref()
            .map(|ct| {
                ct.starts_with("text/")
                    || ct.contains("json")
                    || ct.contains("xml")
                    || ct.contains("javascript")
                    || ct == "application/csv"
            })
            .unwrap_or(false);

        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        let mut action = GetObject::new(&bucket, Some(&creds), key);
        action
            .headers_mut()
            .insert("range", format!("bytes=0-{}", PREVIEW_BYTES - 1));
        let signed = action.sign(SIGN_SECS);
        let response = self
            .http
            .get(signed)
            .header("range", format!("bytes=0-{}", PREVIEW_BYTES - 1))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() && response.status().as_u16() != 206 {
            return Err(format!("GetObject failed ({})", response.status()));
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        let truncated = size as u64 > PREVIEW_BYTES || bytes.len() as u64 >= PREVIEW_BYTES;

        let (text, data_base64) = if is_text {
            (
                Some(String::from_utf8_lossy(&bytes).to_string()),
                None,
            )
        } else if is_image {
            (None, Some(base64_encode(&bytes)))
        } else {
            (None, None)
        };

        Ok(S3ObjectPreview {
            key: key.to_string(),
            content_type,
            size,
            last_modified: meta.and_then(|m| m.last_modified),
            is_text,
            is_image,
            text,
            data_base64,
            truncated,
        })
    }

    pub async fn download_object(
        &self,
        bucket_name: &str,
        key: &str,
        dest: &Path,
    ) -> Result<(), String> {
        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        let action = GetObject::new(&bucket, Some(&creds), key);
        let signed = action.sign(SIGN_SECS);
        let response = self
            .http
            .get(signed)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("Download failed ({})", response.status()));
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(dest, bytes).map_err(|e| e.to_string())
    }

    pub async fn upload_file(
        &self,
        bucket_name: &str,
        key: &str,
        file_path: &Path,
    ) -> Result<(), String> {
        let bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        if bytes.len() > MULTIPART_THRESHOLD {
            self.upload_multipart(bucket_name, key, &bytes).await
        } else {
            self.put_bytes(bucket_name, key, bytes).await
        }
    }

    async fn put_bytes(&self, bucket_name: &str, key: &str, bytes: Vec<u8>) -> Result<(), String> {
        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        let action = PutObject::new(&bucket, Some(&creds), key);
        let signed = action.sign(SIGN_SECS);
        let response = self
            .http
            .put(signed)
            .body(bytes)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Upload failed: {body}"));
        }
        Ok(())
    }

    async fn upload_multipart(
        &self,
        bucket_name: &str,
        key: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        let create = CreateMultipartUpload::new(&bucket, Some(&creds), key);
        let signed = create.sign(SIGN_SECS);
        let response = self
            .http
            .post(signed)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body = response.text().await.map_err(|e| e.to_string())?;
        let upload_id = extract_xml_tag(&body, "UploadId")
            .ok_or_else(|| format!("CreateMultipartUpload failed: {body}"))?;

        let mut etags = Vec::new();
        let mut part_number = 1u16;
        for chunk in bytes.chunks(PART_SIZE) {
            let action = UploadPart::new(&bucket, Some(&creds), key, part_number, &upload_id);
            let signed = action.sign(SIGN_SECS);
            let response = self
                .http
                .put(signed)
                .body(chunk.to_vec())
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!("UploadPart {part_number} failed ({})", response.status()));
            }
            let etag = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim_matches('"')
                .to_string();
            etags.push((part_number, etag));
            part_number += 1;
        }

        let etag_values: Vec<String> = etags.iter().map(|(_, e)| e.clone()).collect();
        let etag_iter = etag_values.iter().map(|e| e.as_str());
        let complete =
            CompleteMultipartUpload::new(&bucket, Some(&creds), key, &upload_id, etag_iter);
        let signed = complete.sign(SIGN_SECS);
        let xml = complete.body();
        let response = self
            .http
            .post(signed)
            .header("content-type", "application/xml")
            .body(xml)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("CompleteMultipartUpload failed: {body}"));
        }
        Ok(())
    }

    pub async fn delete_objects(&self, bucket_name: &str, keys: &[String]) -> Result<(), String> {
        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        for key in keys {
            let action = DeleteObject::new(&bucket, Some(&creds), key);
            let signed = action.sign(SIGN_SECS);
            let response = self
                .http
                .delete(signed)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() && response.status().as_u16() != 204 {
                return Err(format!("DeleteObject failed for {key} ({})", response.status()));
            }
        }
        Ok(())
    }

    pub async fn copy_object(
        &self,
        bucket_name: &str,
        from_key: &str,
        to_key: &str,
    ) -> Result<(), String> {
        let creds = self.credentials();
        let bucket = self.bucket(bucket_name)?;
        let mut action = rusty_s3::actions::PutObject::new(&bucket, Some(&creds), to_key);
        let copy_source = format!("/{bucket_name}/{from_key}");
        action
            .headers_mut()
            .insert("x-amz-copy-source", copy_source.clone());
        let signed = action.sign(SIGN_SECS);
        let response = self
            .http
            .put(signed)
            .header("x-amz-copy-source", copy_source)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("CopyObject failed: {body}"));
        }
        Ok(())
    }

    pub async fn create_folder(&self, bucket_name: &str, prefix: &str) -> Result<(), String> {
        let key = if prefix.ends_with('/') {
            prefix.to_string()
        } else {
            format!("{prefix}/")
        };
        self.put_bytes(bucket_name, &key, Vec::new()).await
    }
}

pub fn default_download_tmp_dir() -> PathBuf {
    std::env::temp_dir().join("dbcooper")
}

pub fn resolve_download_tmp_dir(configured: Option<&str>) -> PathBuf {
    match configured.map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) => expand_tilde(path),
        None => default_download_tmp_dir(),
    }
}

pub fn object_file_name(key: &str) -> String {
    let name = key
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or("download");
    if name == "." || name == ".." {
        "download".to_string()
    } else {
        name.replace('\0', "_")
    }
}

pub fn unique_download_path(dir: &Path, key: &str) -> PathBuf {
    let name = object_file_name(key);
    let dest = dir.join(&name);
    if !dest.exists() {
        return dest;
    }
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());
    let ext = dest.extension().map(|s| s.to_string_lossy().into_owned());
    for i in 1..1000 {
        let mut candidate = dir.join(format!("{stem} ({i})"));
        if let Some(ext) = &ext {
            candidate.set_extension(ext);
        }
        if !candidate.exists() {
            return candidate;
        }
    }
    dest
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

pub fn open_downloaded_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.display().to_string()])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn format_s3_error(status: u16, body: &str) -> String {
    if let Some(message) = extract_xml_tag(body, "Message") {
        return format!("S3 error ({status}): {message}");
    }
    format!("S3 error ({status}): {body}")
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

fn decode_s3_key(value: &str) -> String {
    urlencoding::decode(value)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| value.to_string())
}

fn parse_list_objects(xml: &str) -> Result<S3ListResult, String> {
    let encoded = extract_xml_tag(xml, "EncodingType")
        .is_some_and(|s| s.eq_ignore_ascii_case("url"));
    let decode = |value: String| {
        if encoded {
            decode_s3_key(&value)
        } else {
            value
        }
    };
    let mut objects = Vec::new();
    let mut prefixes = Vec::new();
    for chunk in xml.split("<Contents>").skip(1) {
        let key = extract_xml_tag(&format!("<Contents>{chunk}"), "Key").unwrap_or_default();
        if key.is_empty() {
            continue;
        }
        let size = extract_xml_tag(&format!("<Contents>{chunk}"), "Size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        objects.push(S3Object {
            key: decode(key),
            size,
            last_modified: extract_xml_tag(&format!("<Contents>{chunk}"), "LastModified"),
            etag: extract_xml_tag(&format!("<Contents>{chunk}"), "ETag"),
            storage_class: extract_xml_tag(&format!("<Contents>{chunk}"), "StorageClass"),
        });
    }
    for chunk in xml.split("<CommonPrefixes>").skip(1) {
        if let Some(prefix) = extract_xml_tag(&format!("<CommonPrefixes>{chunk}"), "Prefix") {
            let prefix = decode(prefix);
            if prefix.is_empty() || prefix == "/" {
                continue;
            }
            prefixes.push(S3Prefix { prefix });
        }
    }
    let is_truncated = extract_xml_tag(xml, "IsTruncated")
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    Ok(S3ListResult {
        prefixes,
        objects,
        is_truncated,
        next_continuation_token: extract_xml_tag(xml, "NextContinuationToken"),
    })
}

fn guess_content_type(key: &str) -> Option<String> {
    let ext = key.rsplit('.').next()?.to_lowercase();
    Some(
        match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "txt" | "log" | "md" => "text/plain",
            "json" => "application/json",
            "csv" => "text/csv",
            "xml" => "application/xml",
            "html" | "htm" => "text/html",
            "js" => "text/javascript",
            "css" => "text/css",
            "yaml" | "yml" => "text/yaml",
            _ => return None,
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        object_file_name, parse_list_objects, resolve_download_tmp_dir, unique_download_path,
    };
    use std::fs;

    #[test]
    fn parses_prefixes_and_objects() {
        let xml = r#"
        <ListBucketResult>
          <IsTruncated>false</IsTruncated>
          <Contents>
            <Key>photos/cat.png</Key>
            <Size>12</Size>
            <LastModified>2026-01-01T00:00:00.000Z</LastModified>
          </Contents>
          <CommonPrefixes>
            <Prefix>photos/</Prefix>
          </CommonPrefixes>
        </ListBucketResult>
        "#;
        let result = parse_list_objects(xml).unwrap();
        assert_eq!(result.objects[0].key, "photos/cat.png");
        assert_eq!(result.objects[0].size, 12);
        assert_eq!(result.prefixes[0].prefix, "photos/");
        assert!(!result.is_truncated);
    }

    #[test]
    fn decodes_url_encoded_prefixes_and_keys() {
        let xml = r#"
        <ListBucketResult>
          <EncodingType>url</EncodingType>
          <IsTruncated>false</IsTruncated>
          <Contents>
            <Key>artifacts%2Freadme.txt</Key>
            <Size>4</Size>
          </Contents>
          <CommonPrefixes>
            <Prefix>artifacts%2F</Prefix>
          </CommonPrefixes>
          <CommonPrefixes>
            <Prefix>plans%2F</Prefix>
          </CommonPrefixes>
        </ListBucketResult>
        "#;
        let result = parse_list_objects(xml).unwrap();
        assert_eq!(result.prefixes[0].prefix, "artifacts/");
        assert_eq!(result.prefixes[1].prefix, "plans/");
        assert_eq!(result.objects[0].key, "artifacts/readme.txt");
    }

    #[test]
    fn parses_real_r2_url_encoded_common_prefixes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>bizpulse-artifacts</Name><IsTruncated>false</IsTruncated><CommonPrefixes><Prefix>artifacts%2F</Prefix></CommonPrefixes><CommonPrefixes><Prefix>plans%2F</Prefix></CommonPrefixes><CommonPrefixes><Prefix>strategy-migration%2F</Prefix></CommonPrefixes><CommonPrefixes><Prefix>strategy-migrations%2F</Prefix></CommonPrefixes><Delimiter>%2F</Delimiter><MaxKeys>1000</MaxKeys><KeyCount>4</KeyCount><EncodingType>url</EncodingType></ListBucketResult>"#;
        let result = parse_list_objects(xml).unwrap();
        assert_eq!(
            result
                .prefixes
                .iter()
                .map(|p| p.prefix.as_str())
                .collect::<Vec<_>>(),
            [
                "artifacts/",
                "plans/",
                "strategy-migration/",
                "strategy-migrations/"
            ]
        );
    }

    #[test]
    fn object_file_name_uses_last_segment() {
        assert_eq!(object_file_name("artifacts/plans/notes.md"), "notes.md");
        assert_eq!(object_file_name("notes.md"), "notes.md");
        assert_eq!(object_file_name("folder/"), "folder");
        assert_eq!(object_file_name(".."), "download");
    }

    #[test]
    fn unique_download_path_increments_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_download_path(dir.path(), "photo.png");
        fs::write(&first, b"a").unwrap();
        let second = unique_download_path(dir.path(), "photo.png");
        assert_eq!(second.file_name().unwrap(), "photo (1).png");
    }

    #[test]
    fn resolves_configured_download_dir() {
        let resolved = resolve_download_tmp_dir(Some("/tmp/custom-dbcooper"));
        assert_eq!(resolved, std::path::PathBuf::from("/tmp/custom-dbcooper"));
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 63) as usize] as char);
        out.push(TABLE[((triple >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
