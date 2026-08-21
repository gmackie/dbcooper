use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::time::Instant;

use super::json_rows::{normalize_filter, obj_i64, obj_opt_str, obj_str, quote_ident};
use super::{query_returns_rows, DatabaseDriver};
use crate::db::models::{
    ColumnInfo, ForeignKeyInfo, IndexInfo, QueryResult, SchemaOverview, TableDataResponse,
    TableInfo, TableStructure, TableWithStructure, TestConnectionResult,
};

#[derive(Clone)]
pub struct TursoConfig {
    pub url: String,
    pub auth_token: String,
}

pub struct TursoDriver {
    config: TursoConfig,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct PipelineResponse {
    #[serde(default)]
    results: Vec<PipelineResult>,
}

#[derive(Debug, Deserialize)]
struct PipelineResult {
    #[serde(rename = "type")]
    result_type: String,
    #[serde(default)]
    response: Option<PipelineInner>,
    #[serde(default)]
    error: Option<PipelineError>,
}

#[derive(Debug, Deserialize)]
struct PipelineInner {
    #[serde(rename = "type")]
    inner_type: String,
    #[serde(default)]
    result: Option<ExecuteResult>,
}

#[derive(Debug, Deserialize)]
struct PipelineError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteResult {
    #[serde(default)]
    cols: Vec<Col>,
    #[serde(default)]
    rows: Vec<Vec<HranaValue>>,
    #[serde(default)]
    affected_row_count: u64,
}

#[derive(Debug, Deserialize)]
struct Col {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HranaValue {
    #[serde(rename = "type")]
    value_type: String,
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    base64: Option<String>,
}

impl TursoDriver {
    pub fn new(config: TursoConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn normalize_url(url: &str) -> String {
        let trimmed = url.trim().trim_end_matches('/');
        let https = if let Some(rest) = trimmed.strip_prefix("libsql://") {
            format!("https://{rest}")
        } else if let Some(rest) = trimmed.strip_prefix("libsql+https://") {
            format!("https://{rest}")
        } else {
            trimmed.to_string()
        };
        if https.ends_with("/v2/pipeline") {
            https
        } else {
            format!("{https}/v2/pipeline")
        }
    }

    async fn execute_sql(&self, sql: &str) -> Result<ExecuteResult, String> {
        let url = Self::normalize_url(&self.config.url);
        let body = json!({
            "requests": [
                { "type": "execute", "stmt": { "sql": sql } },
                { "type": "close" }
            ]
        });
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.config.auth_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        let text = response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format_turso_http_error(&text, status.as_u16()));
        }
        let pipeline: PipelineResponse =
            serde_json::from_str(&text).map_err(|e| format!("Invalid Turso response: {e}"))?;
        let first = pipeline
            .results
            .into_iter()
            .next()
            .ok_or_else(|| "Turso returned no result".to_string())?;
        if first.result_type == "error" {
            return Err(first
                .error
                .map(|e| e.message)
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "Turso statement failed".to_string()));
        }
        let inner = first
            .response
            .ok_or_else(|| "Turso returned an empty response".to_string())?;
        if inner.inner_type != "execute" {
            return Ok(ExecuteResult {
                cols: vec![],
                rows: vec![],
                affected_row_count: 0,
            });
        }
        inner
            .result
            .ok_or_else(|| "Turso execute returned no result".to_string())
    }

    async fn query_rows(&self, sql: &str) -> Result<Vec<Value>, String> {
        let result = self.execute_sql(sql).await?;
        Ok(hrana_rows_to_objects(&result))
    }
}

fn format_turso_http_error(body: &str, status: u16) -> String {
    if body.contains("platform") || body.to_lowercase().contains("unauthorized") {
        return format!(
            "Turso request failed ({status}). Use a database auth token from `turso db tokens create`, not the platform API token."
        );
    }
    format!("Turso API error ({status}): {body}")
}

fn hrana_value_to_json(value: &HranaValue) -> Value {
    match value.value_type.as_str() {
        "null" => Value::Null,
        "integer" => value
            .value
            .as_ref()
            .and_then(|v| {
                v.as_i64()
                    .map(Value::from)
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()).map(Value::from))
            })
            .unwrap_or(Value::Null),
        "float" => value
            .value
            .as_ref()
            .and_then(|v| {
                v.as_f64()
                    .map(Value::from)
                    .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()).map(Value::from))
            })
            .unwrap_or(Value::Null),
        "text" => value
            .value
            .as_ref()
            .and_then(|v| v.as_str().map(|s| Value::String(s.to_string())))
            .unwrap_or(Value::Null),
        "blob" => {
            let encoded = value
                .base64
                .clone()
                .or_else(|| value.value.as_ref().and_then(|v| v.as_str().map(|s| s.to_string())))
                .unwrap_or_default();
            Value::String(format!("[{} bytes blob]", encoded.len()))
        }
        _ => value.value.clone().unwrap_or(Value::Null),
    }
}

fn hrana_rows_to_objects(result: &ExecuteResult) -> Vec<Value> {
    let names: Vec<String> = result
        .cols
        .iter()
        .enumerate()
        .map(|(i, col)| {
            col.name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("col_{i}"))
        })
        .collect();
    result
        .rows
        .iter()
        .map(|row| {
            let mut obj = Map::new();
            for (i, value) in row.iter().enumerate() {
                let key = names.get(i).cloned().unwrap_or_else(|| format!("col_{i}"));
                obj.insert(key, hrana_value_to_json(value));
            }
            Value::Object(obj)
        })
        .collect()
}

fn table_info_from_rows(rows: &[Value]) -> Vec<TableInfo> {
    rows.iter()
        .map(|row| TableInfo {
            schema: "main".to_string(),
            name: obj_str(row, "name"),
            table_type: obj_str(row, "type"),
        })
        .filter(|t| !t.name.is_empty())
        .collect()
}

fn columns_from_pragma(rows: &[Value]) -> Vec<ColumnInfo> {
    rows.iter()
        .map(|row| ColumnInfo {
            name: obj_str(row, "name"),
            data_type: obj_str(row, "type").to_uppercase(),
            nullable: obj_i64(row, "notnull") == 0,
            default: obj_opt_str(row, "dflt_value"),
            primary_key: obj_i64(row, "pk") > 0,
        })
        .collect()
}

#[async_trait]
impl DatabaseDriver for TursoDriver {
    async fn test_connection(&self) -> Result<TestConnectionResult, String> {
        match self.query_rows("SELECT 1 AS ok").await {
            Ok(_) => Ok(TestConnectionResult {
                success: true,
                message: "Connection successful!".to_string(),
            }),
            Err(e) => Ok(TestConnectionResult {
                success: false,
                message: format!("Connection failed: {e}"),
            }),
        }
    }

    async fn list_tables(&self) -> Result<Vec<TableInfo>, String> {
        let rows = self
            .query_rows(
                "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .await?;
        Ok(table_info_from_rows(&rows))
    }

    async fn get_table_data(
        &self,
        _schema: &str,
        table: &str,
        page: i64,
        limit: i64,
        filter: Option<String>,
        sort_column: Option<String>,
        sort_direction: Option<String>,
    ) -> Result<TableDataResponse, String> {
        let ident = quote_ident(table);
        let where_clause = filter
            .as_ref()
            .map(|f| format!(" WHERE {}", normalize_filter(f)))
            .unwrap_or_default();
        let order_clause = if let Some(col) = sort_column.as_ref() {
            let dir = match sort_direction
                .as_deref()
                .map(|s| s.to_lowercase())
                .as_deref()
            {
                Some("desc") => "DESC",
                _ => "ASC",
            };
            format!(" ORDER BY {} {dir}", quote_ident(col))
        } else {
            String::new()
        };
        let offset = (page - 1) * limit;
        let count_rows = self
            .query_rows(&format!(
                "SELECT COUNT(*) AS count FROM {ident}{where_clause}"
            ))
            .await?;
        let total = count_rows.first().map(|row| obj_i64(row, "count")).unwrap_or(0);
        let data = self
            .query_rows(&format!(
                "SELECT * FROM {ident}{where_clause}{order_clause} LIMIT {limit} OFFSET {offset}"
            ))
            .await?;
        Ok(TableDataResponse {
            data,
            total,
            page,
            limit,
        })
    }

    async fn get_table_structure(
        &self,
        _schema: &str,
        table: &str,
    ) -> Result<TableStructure, String> {
        let ident = quote_ident(table);
        let columns =
            columns_from_pragma(&self.query_rows(&format!("PRAGMA table_info({ident})")).await?);
        let index_list = self
            .query_rows(&format!("PRAGMA index_list({ident})"))
            .await
            .unwrap_or_default();
        let mut indexes = Vec::new();
        for idx in &index_list {
            let idx_name = obj_str(idx, "name");
            if idx_name.is_empty() {
                continue;
            }
            let idx_cols = self
                .query_rows(&format!("PRAGMA index_info({})", quote_ident(&idx_name)))
                .await
                .unwrap_or_default();
            indexes.push(IndexInfo {
                name: idx_name,
                columns: idx_cols.iter().map(|row| obj_str(row, "name")).collect(),
                unique: obj_i64(idx, "unique") == 1,
                primary: obj_str(idx, "origin") == "pk",
            });
        }
        let fk_rows = self
            .query_rows(&format!("PRAGMA foreign_key_list({ident})"))
            .await
            .unwrap_or_default();
        let foreign_keys = fk_rows
            .iter()
            .map(|row| ForeignKeyInfo {
                name: format!("fk_{}", obj_i64(row, "id")),
                column: obj_str(row, "from"),
                references_table: obj_str(row, "table"),
                references_column: obj_str(row, "to"),
            })
            .collect();
        Ok(TableStructure {
            columns,
            indexes,
            foreign_keys,
        })
    }

    async fn execute_query(&self, query: &str) -> Result<QueryResult, String> {
        let start = Instant::now();
        match self.execute_sql(query).await {
            Ok(result) => {
                let data = hrana_rows_to_objects(&result);
                let returns_rows = query_returns_rows(query);
                Ok(QueryResult {
                    row_count: if returns_rows {
                        data.len() as i64
                    } else {
                        result.affected_row_count as i64
                    },
                    data,
                    rows_affected: if returns_rows {
                        None
                    } else {
                        Some(result.affected_row_count)
                    },
                    error: None,
                    time_taken_ms: Some(start.elapsed().as_millis()),
                })
            }
            Err(e) => Ok(QueryResult {
                data: vec![],
                row_count: 0,
                rows_affected: None,
                error: Some(e),
                time_taken_ms: Some(start.elapsed().as_millis()),
            }),
        }
    }

    async fn get_schema_overview(&self) -> Result<SchemaOverview, String> {
        let tables = self.list_tables().await?;
        let mut overview_tables = Vec::new();
        for table in tables {
            let structure = self
                .get_table_structure("main", &table.name)
                .await
                .unwrap_or(TableStructure {
                    columns: Vec::new(),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                });
            overview_tables.push(TableWithStructure {
                schema: table.schema,
                name: table.name,
                table_type: table.table_type,
                columns: structure.columns,
                foreign_keys: structure.foreign_keys,
                indexes: structure.indexes,
            });
        }
        Ok(SchemaOverview {
            tables: overview_tables,
            functions: Vec::new(),
        })
    }
}
