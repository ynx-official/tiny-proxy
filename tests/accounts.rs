use serde_json::json;
use tiny_proxy::{
    auth::codex::{Service, now},
    platform::protection,
    storage::Store,
};

#[tokio::test]
async fn legacy_session_is_imported_once_and_survives_account_switches() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let bundle = json!({"access_token":"fixture","refresh_token":"fixture","account_id":"old-account","subject":"old-user","expires_at":now()+3600});
    let blob = protection::protect(bundle.to_string().as_bytes()).unwrap();
    store
        .save_codex_credentials(0, &blob, now() + 3600)
        .await
        .unwrap();
    let service = Service::new(store.clone()).unwrap();
    service.initialize().await.unwrap();
    let first = store.accounts().await.unwrap()[0].id.clone();
    assert_eq!(
        store.account_credentials(&first).await.unwrap().unwrap().1,
        blob
    );
    store
        .upsert_account("two", "Second", b"fixture-two", 100, true)
        .await
        .unwrap();
    service.initialize().await.unwrap();
    assert_eq!(store.accounts().await.unwrap().len(), 2);
    assert_eq!(
        store.active_account().await.unwrap().as_deref(),
        Some("two")
    );
    service.activate(&first).await.unwrap();
    assert_eq!(
        store.active_account().await.unwrap().as_deref(),
        Some(first.as_str())
    );
}
