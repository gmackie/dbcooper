use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

use super::json_rows::{normalize_filter, obj_i64, obj_opt_str, obj_str, quote_ident};
use super::{query_returns_rows, DatabaseDriver};
use crate::db::models::{
    ColumnInfo, ForeignKeyInfo, IndexInfo, QueryResult, SchemaOverview, TableDataResponse,
    TableInfo, TableStructure, TableWithStructure, TestConnectionResult,
};

#[derive(Clone)]
pub struct D1Config {
    pub account_id: String,
    pub database_id: String,
    pub api_token: String,
    pub api_base: Option<String>,
}

pub struct D1Driver {
    config: D1Config,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct CfEnvelope {
    success: bool,
    #[serde(default)]
    errors: Vec<CfError>,
    #[serde(default)]
    result: Vec<D1Statement>,
}

#[derive(Debug, Deserialize)]
struct CfError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct D1Statement {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    results: Vec<Value>,
    #[serde(default)]
    meta: D1Meta,
}

#[derive(Debug, Default, Deserialize)]
struct D1Meta {
    #[serde(default)]
    changes: Option<f64>,
    #[serde(default)]
    duration: Option<f64>,
}

impl D1Driver {
    pub fn new(config: D1Config) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    fn query_url(&self) -> String {
        let base = self
            .config
            .api_base
            .as_deref()
            .unwrap_or("https://api.cloudflare.com/client/v4");
        format!(
            "{}/accounts/{}/d1/database/{}/query",
            base.trim_end_matches('/'),
            self.config.account_id,
            self.config.database_id
        )
    }

    async fn query_statement(&self, sql: &str) -> Result<D1Statement, String> {
        let response = self
            .client
            .post(self.query_url())
            .bearer_auth(&self.config.api_token)
            .json(&json!({ "sql": sql }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = response.status();
        let body = response.text().await.map_err(|e| e.to_string())?;
        let envelope: CfEnvelope = serde_json::from_str(&body).map_err(|e| {
            if !status.is_success() {
                format!("D1 API error ({status}): {body}")
            } else {
                format!("Invalid D1 response: {e}")
            }
        })?;

        if !envelope.success {
            let message = envelope
                .errors
                .into_iter()
                .map(|err| err.message)
                .filter(|m| !m.is_empty())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(if message.is_empty() {
                format!("D1 query failed ({status}): {body}")
            } else {
                message
            });
        }

        envelope
            .result
            .into_iter()
            .next()
            .ok_or_else(|| "D1 returned no result".to_string())
            .and_then(|stmt| {
                if !stmt.success && stmt.results.is_empty() {
                    Err("D1 statement failed".to_string())
                } else {
                    Ok(stmt)
                }
            })
    }

    async fn query_rows(&self, sql: &str) -> Result<Vec<Value>, String> {
        Ok(self.query_statement(sql).await?.results)
    }
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
impl DatabaseDriver for D1Driver {
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
        let columns = columns_from_pragma(&self.query_rows(&format!("PRAGMA table_info({ident})")).await?);

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
        match self.query_statement(query).await {
            Ok(stmt) => {
                let rows_affected = stmt.meta.changes.map(|c| c as u64);
                let data = stmt.results;
                let row_count = if query_returns_rows(query) {
                    data.len() as i64
                } else {
                    rows_affected.unwrap_or(0) as i64
                };
                Ok(QueryResult {
                    data,
                    row_count,
                    rows_affected: if query_returns_rows(query) {
                        None
                    } else {
                        rows_affected
                    },
                    error: None,
                    time_taken_ms: Some(
                        stmt.meta
                            .duration
                            .map(|d| d as u128)
                            .unwrap_or_else(|| start.elapsed().as_millis()),
                    ),
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
