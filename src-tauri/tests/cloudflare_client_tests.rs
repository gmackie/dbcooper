use dbcooper_lib::cloudflare::CloudflareClient;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn verify_and_list_resources() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/tokens/verify"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "result": { "id": "token-id", "status": "active" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/accounts/acc/d1/database"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "result": [{ "uuid": "db-1", "name": "prod" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/accounts/acc/r2/buckets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "result": { "buckets": [{ "name": "assets" }] }
        })))
        .mount(&server)
        .await;

    let client = CloudflareClient::new("tok").with_api_base(server.uri());
    let verify = client.verify_token().await.unwrap();
    assert_eq!(verify.id, "token-id");
    let dbs = client.list_d1_databases("acc").await.unwrap();
    assert_eq!(dbs[0].name, "prod");
    let buckets = client.list_r2_buckets("acc").await.unwrap();
    assert_eq!(buckets[0].name, "assets");
    let creds = client.derive_r2_credentials(&verify.id);
    assert_eq!(creds.access_key_id, "token-id");
    assert_eq!(creds.secret_access_key.len(), 64);
}
