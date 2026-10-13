use tiny_proxy::auth::loopback::Listener;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn loopback_rejects_bad_state_accepts_one_callback_and_releases_port() {
    let listener = Listener::bind("urn:uuid:test-host", None).await.unwrap();
    let port = listener.port;
    let state = listener
        .authorization_url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();
    let url = format!("http://127.0.0.1:{port}/auth/callback");
    let client = reqwest::Client::new();
    let wrong = client
        .get(format!("{url}?code=x&client_id=oaiapp_test&state=wrong"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 400);
    let callback = format!("{url}?code=synthetic-code&client_id=oaiapp_test&state={state}");
    let success = client.get(&callback).send().await.unwrap();
    assert_eq!(success.status(), 200);
    assert!(success.headers().contains_key("content-security-policy"));
    let html = success.text().await.unwrap();
    assert!(!html.contains("synthetic-code"));
    assert!(!html.contains(&state));
    assert_eq!(client.get(callback).send().await.unwrap().status(), 400);
    let grant = listener.wait(CancellationToken::new()).await.unwrap();
    assert_eq!(grant.client_id, "oaiapp_test");
    assert!(
        tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn cancelling_wait_closes_the_callback_listener() {
    let listener = Listener::bind("urn:uuid:test-host", None).await.unwrap();
    let port = listener.port;
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(listener.wait(cancel).await.is_err());
    assert!(
        tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .is_ok()
    );
}
