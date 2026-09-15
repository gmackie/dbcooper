use dbcooper_lib::cloudflare::{
    is_account_api_token, normalize_api_token, CloudflareClient,
};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn invalid_token() -> ResponseTemplate {
    ResponseTemplate::new(401).set_body_json(json!({
        "success": false,
        "errors": [{ "code": 1000, "message": "Invalid API Token" }]
    }))
}

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

#[test]
fn account_token_prefix_and_normalize() {
    assert!(is_account_api_token("cfat_abc"));
    assert!(is_account_api_token("  Bearer cfat_abc  "));
    assert!(!is_account_api_token("cfut_abc"));
    assert_eq!(normalize_api_token("  Bearer tok  "), "tok");
}

#[tokio::test]
async fn account_token_skips_user_verify() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/tokens/verify"))
        .respond_with(invalid_token())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/accounts"))
        .and(header("authorization", "Bearer cfat_live"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "result": [{ "id": "acc-1", "name": "Prod" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/accounts/acc-1/tokens/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "result": { "id": "acct-token-id", "status": "active" }
        })))
        .mount(&server)
        .await;

    let client = CloudflareClient::new("  cfat_live  ").with_api_base(server.uri());
    let auth = client.authenticate(None).await.unwrap();
    assert_eq!(auth.token_id.as_deref(), Some("acct-token-id"));
    assert_eq!(auth.accounts[0].id, "acc-1");
}

#[tokio::test]
async fn account_token_with_account_id_verifies_account_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/tokens/verify"))
        .respond_with(invalid_token())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/accounts/acc-1/tokens/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "result": { "id": "acct-token-id", "status": "active" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/accounts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "result": [{ "id": "acc-1", "name": "Prod" }]
        })))
        .mount(&server)
        .await;

    let client = CloudflareClient::new("cfat_live").with_api_base(server.uri());
    let auth = client.authenticate(Some("acc-1")).await.unwrap();
    assert_eq!(auth.token_id.as_deref(), Some("acct-token-id"));
}

#[tokio::test]
async fn bogus_user_token_still_reports_invalid() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user/tokens/verify"))
        .respond_with(invalid_token())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/accounts"))
        .respond_with(invalid_token())
        .mount(&server)
        .await;

    let client = CloudflareClient::new("not-a-real-token").with_api_base(server.uri());
    let err = client.authenticate(None).await.unwrap_err();
    assert!(err.contains("Invalid API Token"), "{err}");
}
