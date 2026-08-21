use dbcooper_lib::database::d1::{D1Config, D1Driver};
use dbcooper_lib::database::DatabaseDriver;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn d1_result(rows: serde_json::Value) -> serde_json::Value {
    json!({
        "success": true,
        "errors": [],
        "result": [{
            "success": true,
            "results": rows,
            "meta": { "changes": 0, "duration": 1.5 }
        }]
    })
}

async fn driver(server: &MockServer) -> D1Driver {
    D1Driver::new(D1Config {
        account_id: "acc".into(),
        database_id: "db".into(),
        api_token: "token".into(),
        api_base: Some(format!("{}/client/v4", server.uri())),
    })
}

#[tokio::test]
async fn d1_test_connection_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/client/v4/accounts/acc/d1/database/db/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(d1_result(json!([{"ok": 1}]))))
        .mount(&server)
        .await;

    let result = driver(&server).await.test_connection().await.unwrap();
    assert!(result.success);
}

#[tokio::test]
async fn d1_list_tables_and_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/client/v4/accounts/acc/d1/database/db/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(d1_result(json!([
            {"name": "users", "type": "table"}
        ]))))
        .mount(&server)
        .await;

    let tables = driver(&server).await.list_tables().await.unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].name, "users");
}

#[tokio::test]
async fn d1_execute_error_is_query_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/client/v4/accounts/acc/d1/database/db/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": false,
            "errors": [{"message": "D1_ERROR: no such table: missing"}],
            "result": []
        })))
        .mount(&server)
        .await;

    let result = driver(&server)
        .await
        .execute_query("SELECT * FROM missing")
        .await
        .unwrap();
    assert!(result.error.unwrap().contains("no such table"));
}
