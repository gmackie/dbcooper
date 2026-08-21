use dbcooper_lib::database::turso::{TursoConfig, TursoDriver};
use dbcooper_lib::database::DatabaseDriver;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pipeline_rows(cols: &[&str], rows: Vec<Vec<serde_json::Value>>) -> serde_json::Value {
    json!({
        "results": [{
            "type": "ok",
            "response": {
                "type": "execute",
                "result": {
                    "cols": cols.iter().map(|name| json!({"name": name})).collect::<Vec<_>>(),
                    "rows": rows,
                    "affected_row_count": 0
                }
            }
        }, { "type": "ok", "response": { "type": "close" } }]
    })
}

#[tokio::test]
async fn turso_select_maps_typed_values() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/pipeline"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pipeline_rows(
            &["id", "name"],
            vec![vec![
                json!({"type": "integer", "value": "42"}),
                json!({"type": "text", "value": "ada"}),
            ]],
        )))
        .mount(&server)
        .await;

    let driver = TursoDriver::new(TursoConfig {
        url: server.uri(),
        auth_token: "db-token".into(),
    });
    let result = driver.execute_query("SELECT id, name FROM users").await.unwrap();
    assert!(result.error.is_none());
    assert_eq!(result.row_count, 1);
    assert_eq!(result.data[0]["id"], 42);
    assert_eq!(result.data[0]["name"], "ada");
}

#[tokio::test]
async fn turso_normalizes_libsql_url() {
    assert_eq!(
        TursoDriver::normalize_url("libsql://example.turso.io"),
        "https://example.turso.io/v2/pipeline"
    );
    assert_eq!(
        TursoDriver::normalize_url("https://example.turso.io/v2/pipeline"),
        "https://example.turso.io/v2/pipeline"
    );
}

#[tokio::test]
async fn turso_error_mentions_database_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/pipeline"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized platform token"))
        .mount(&server)
        .await;

    let driver = TursoDriver::new(TursoConfig {
        url: server.uri(),
        auth_token: "platform".into(),
    });
    let result = driver.test_connection().await.unwrap();
    assert!(!result.success);
    assert!(result.message.contains("database auth token"));
}
