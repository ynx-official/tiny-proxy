use tiny_proxy::{
    auth::codex::now,
    storage::{KeyStatus, Store},
};

#[tokio::test]
async fn old_digest_only_key_remains_valid_but_cannot_be_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    let key = store.create_key("legacy").await.unwrap();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    sqlx::query("UPDATE client_keys SET encrypted_value=NULL WHERE id=?")
        .bind(&key.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!store.list_keys().await.unwrap()[0].recoverable);
    assert!(matches!(
        store.reveal_key(&key.id).await,
        Err(tiny_proxy::storage::Error::KeyUnavailable)
    ));
    assert!(matches!(
        store.authenticate(key.value.as_str()).await.unwrap(),
        KeyStatus::Valid(_)
    ));
    pool.close().await;
    store.close().await;
}

#[tokio::test]
async fn cancelled_probe_retains_a_finished_failure_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    store
        .upsert_account("one", "One", b"unused", 100, true)
        .await
        .unwrap();
    let service = tiny_proxy::auth::codex::Service::new(store.clone()).unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    assert!(
        tiny_proxy::app::test_and_save(&service, &store, "model", cancel)
            .await
            .is_err()
    );
    let tests = store.model_tests("one").await.unwrap();
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].status, "failed");
    assert_eq!(tests[0].error.as_deref(), Some("测试已取消"));
    assert!(tests[0].finished_at.is_some());
}

#[tokio::test]
async fn saved_keys_and_account_state_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    let key = store.create_key("desktop").await.unwrap();
    assert_eq!(
        store.reveal_key(&key.id).await.unwrap().as_str(),
        key.value.as_str()
    );
    store
        .upsert_account("one", "Account one", b"encrypted-one", now() + 3600, true)
        .await
        .unwrap();
    store
        .upsert_account("two", "Account two", b"encrypted-two", now() + 3600, true)
        .await
        .unwrap();
    store
        .save_models(
            "one",
            r#"[{"slug":"m","display_name":"M","reasoning_efforts":[]}]"#,
            100,
        )
        .await
        .unwrap();
    let test = store.start_model_test("one", "m", 101).await.unwrap();
    store
        .finish_model_test(&test, Some("OK"), None, Some(1), Some(2), 102)
        .await
        .unwrap();
    store
        .save_usage("one", Some("{}"), None, 103)
        .await
        .unwrap();
    store.close().await;
    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store.reveal_key(&key.id).await.unwrap().as_str(),
        key.value.as_str()
    );
    assert!(matches!(
        store.authenticate(key.value.as_str()).await.unwrap(),
        KeyStatus::Valid(_)
    ));
    assert_eq!(store.accounts().await.unwrap().len(), 2);
    assert_eq!(
        store.active_account().await.unwrap().as_deref(),
        Some("two")
    );
    assert!(
        store
            .account("one")
            .await
            .unwrap()
            .unwrap()
            .models_json
            .unwrap()
            .contains("\"m\"")
    );
    let tests = store.model_tests("one").await.unwrap();
    assert_eq!(tests[0].text.as_deref(), Some("OK"));
    assert_eq!(tests[0].finished_at, Some(102));
    assert!(store.model_tests("two").await.unwrap().is_empty());
    store.revoke_key(&key.id).await.unwrap();
    assert!(matches!(
        store.authenticate(key.value.as_str()).await.unwrap(),
        KeyStatus::Invalid
    ));
    store.close().await;
    let raw = std::fs::read(path).unwrap();
    assert!(
        !raw.windows(key.value.len())
            .any(|w| w == key.value.as_bytes())
    );
}

#[tokio::test]
async fn account_updates_are_isolated_and_usage_failures_keep_last_success() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    store
        .upsert_account("one", "One", b"a", 100, true)
        .await
        .unwrap();
    store
        .upsert_account("two", "Two", b"b", 100, true)
        .await
        .unwrap();
    store
        .upsert_account("one", "One", b"new-a", 200, true)
        .await
        .unwrap();
    assert_eq!(store.accounts().await.unwrap().len(), 2);
    assert_eq!(
        store.account_credentials("two").await.unwrap().unwrap().1,
        b"b"
    );
    assert!(store.rotate_account("one", 1, b"stale", 200).await.is_err());
    store
        .save_usage("one", Some("{\"remaining\":80}"), None, 1000)
        .await
        .unwrap();
    store
        .save_usage("one", None, Some("网络失败"), 2000)
        .await
        .unwrap();
    let account = store.account("one").await.unwrap().unwrap();
    assert_eq!(account.usage_checked_at, Some(1000));
    assert_eq!(account.usage_attempted_at, Some(2000));
    assert!(account.usage_json.unwrap().contains("80"));
    assert!(
        !store
            .due_accounts(5599)
            .await
            .unwrap()
            .contains(&"one".to_owned())
    );
    assert!(
        store
            .due_accounts(5600)
            .await
            .unwrap()
            .contains(&"one".to_owned())
    );
    let test = store.start_model_test("one", "m", 2000).await.unwrap();
    store.interrupt_model_tests().await.unwrap();
    assert_eq!(store.model_tests("one").await.unwrap()[0].id, test);
    assert_eq!(
        store.model_tests("one").await.unwrap()[0].status,
        "interrupted"
    );
}
