//! Cloudflare API client for D1 discovery, R2 bucket listing, and token verify.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const DEFAULT_API_BASE: &str = "https://api.cloudflare.com/client/v4";

#[derive(Debug, Clone)]
pub struct CloudflareClient {
    pub api_base: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareAccount {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct D1Database {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub file_size: Option<i64>,
    #[serde(default)]
    pub num_tables: Option<i64>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R2Bucket {
    pub name: String,
    #[serde(default)]
    pub creation_date: Option<String>,
    #[serde(default)]
    pub jurisdiction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenVerify {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct CloudflareAuth {
    pub token_id: Option<String>,
    pub accounts: Vec<CloudflareAccount>,
}

pub fn normalize_api_token(token: &str) -> String {
    let token = token.trim();
    let token = token
        .strip_prefix("Bearer ")
        .or_else(|| token.strip_prefix("bearer "))
        .unwrap_or(token);
    token.trim().to_string()
}

pub fn is_account_api_token(token: &str) -> bool {
    normalize_api_token(token).starts_with("cfat_")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R2S3Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[derive(Debug, Deserialize)]
struct CfEnvelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<CfError>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct CfError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct R2BucketsResult {
    #[serde(default)]
    buckets: Vec<R2Bucket>,
}

impl CloudflareClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            api_base: DEFAULT_API_BASE.to_string(),
            token: normalize_api_token(&token.into()),
        }
    }

    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into();
        self
    }

    fn client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, String> {
        let url = format!("{}{}", self.api_base.trim_end_matches('/'), path);
        let response = self
            .client()
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = response.status();
        let body = response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format_cf_error(&body, status.as_u16()));
        }

        let envelope: CfEnvelope<T> =
            serde_json::from_str(&body).map_err(|e| format!("Invalid Cloudflare response: {e}"))?;
        if !envelope.success {
            let message = envelope
                .errors
                .into_iter()
                .map(|err| err.message)
                .filter(|m| !m.is_empty())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(if message.is_empty() {
                "Cloudflare API request failed".to_string()
            } else {
                message
            });
        }
        envelope
            .result
            .ok_or_else(|| "Cloudflare API returned no result".to_string())
    }

    pub async fn verify_token(&self) -> Result<TokenVerify, String> {
        self.get_json("/user/tokens/verify").await
    }

    pub async fn verify_account_token(&self, account_id: &str) -> Result<TokenVerify, String> {
        self.get_json(&format!("/accounts/{account_id}/tokens/verify"))
            .await
    }

    pub async fn list_accounts(&self) -> Result<Vec<CloudflareAccount>, String> {
        self.get_json("/accounts").await
    }

    /// Resolve token identity without assuming a user API token.
    ///
    /// Account tokens (`cfat_…`) fail `GET /user/tokens/verify` with
    /// "Invalid API Token" even when they can list accounts, D1, and R2.
    pub async fn authenticate(&self, account_id: Option<&str>) -> Result<CloudflareAuth, String> {
        let account_id = account_id.map(str::trim).filter(|id| !id.is_empty());

        if let Some(id) = account_id {
            if is_account_api_token(&self.token) {
                let verify = self.verify_account_token(id).await?;
                let accounts = self.list_accounts().await.unwrap_or_default();
                return Ok(CloudflareAuth {
                    token_id: Some(verify.id),
                    accounts,
                });
            }
        }

        match self.verify_token().await {
            Ok(verify) => {
                let accounts = self.list_accounts().await.unwrap_or_default();
                Ok(CloudflareAuth {
                    token_id: Some(verify.id),
                    accounts,
                })
            }
            Err(user_err) => {
                if let Some(id) = account_id {
                    if let Ok(verify) = self.verify_account_token(id).await {
                        let accounts = self.list_accounts().await.unwrap_or_default();
                        return Ok(CloudflareAuth {
                            token_id: Some(verify.id),
                            accounts,
                        });
                    }
                }

                match self.list_accounts().await {
                    Ok(accounts) if !accounts.is_empty() => {
                        let token_id = if let Some(id) = account_id.or(accounts.first().map(|a| a.id.as_str()))
                        {
                            self.verify_account_token(id)
                                .await
                                .ok()
                                .map(|verify| verify.id)
                        } else {
                            None
                        };
                        Ok(CloudflareAuth { token_id, accounts })
                    }
                    Ok(_) | Err(_) => {
                        if is_account_api_token(&self.token) && account_id.is_none() {
                            Err("This Cloudflare token is account-scoped (cfat_). Paste your Account ID from the dashboard URL, then test again.".to_string())
                        } else {
                            Err(user_err)
                        }
                    }
                }
            }
        }
    }

    pub async fn list_d1_databases(&self, account_id: &str) -> Result<Vec<D1Database>, String> {
        self.get_json(&format!("/accounts/{account_id}/d1/database"))
            .await
    }

    pub async fn list_r2_buckets(&self, account_id: &str) -> Result<Vec<R2Bucket>, String> {
        let value: Value = self
            .get_json(&format!("/accounts/{account_id}/r2/buckets"))
            .await?;
        if let Ok(wrapped) = serde_json::from_value::<R2BucketsResult>(value.clone()) {
            return Ok(wrapped.buckets);
        }
        serde_json::from_value(value).map_err(|e| format!("Invalid R2 buckets payload: {e}"))
    }

    pub fn derive_r2_credentials(&self, token_id: &str) -> R2S3Credentials {
        let digest = Sha256::digest(self.token.as_bytes());
        R2S3Credentials {
            access_key_id: token_id.to_string(),
            secret_access_key: hex::encode(digest),
        }
    }
}

fn format_cf_error(body: &str, status: u16) -> String {
    if let Ok(envelope) = serde_json::from_str::<CfEnvelope<Value>>(body) {
        let message = envelope
            .errors
            .into_iter()
            .map(|err| err.message)
            .filter(|m| !m.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        if !message.is_empty() {
            return message;
        }
    }
    format!("Cloudflare API error ({status}): {body}")
}
