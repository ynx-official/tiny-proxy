use tiny_proxy::{
    platform::single_instance::InstanceGuard,
    storage::{Store, recovery::recover_legacy},
};

async fn populated(dir: &std::path::Path) -> Store {
    std::fs::create_dir_all(dir).unwrap();
    let store = Store::open(&dir.join("app.db")).await.unwrap();
    store.create_key("saved-client").await.unwrap();
    store
        .upsert_account("one", "Saved account", b"encrypted", 1, true)
        .await
        .unwrap();
    store
        .save_models(
            "one",
            r#"[{"slug":"m","display_name":"M","reasoning_efforts":[]}]"#,
            123,
        )
        .await
        .unwrap();
    let test = store.start_model_test("one", "m", 124).await.unwrap();
    store
        .finish_model_test(&test, Some("saved result"), None, Some(1), Some(2), 125)
        .await
        .unwrap();
    store
}

#[tokio::test]
async fn restores_populated_redirected_database_including_wal_and_keeps_source() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("Local/TinyProxy");
    std::fs::create_dir_all(&empty).unwrap();
    Store::open(&empty.join("app.db"))
        .await
        .unwrap()
        .close()
        .await;
    let source = root
        .path()
        .join("Packages/example/LocalCache/Local/TinyProxy");
    // Keep the pool open: recovery must include committed WAL pages.
    let original = populated(&source).await;
    assert!(source.join("app.db-wal").exists());
    let target_dir = root.path().join(".tiny-proxy");
    std::fs::create_dir(&target_dir).unwrap();
    let target = target_dir.join("app.db");
    assert!(
        recover_legacy(&target, &[empty, source.clone(), source.join(".")])
            .await
            .unwrap()
            .is_some()
    );
    let restored = Store::open(&target).await.unwrap();
    assert_eq!(restored.list_keys().await.unwrap().len(), 1);
    assert_eq!(restored.accounts().await.unwrap().len(), 1);
    assert_eq!(
        restored
            .account("one")
            .await
            .unwrap()
            .unwrap()
            .models_checked_at,
        Some(123)
    );
    assert_eq!(
        restored.model_tests("one").await.unwrap()[0]
            .text
            .as_deref(),
        Some("saved result")
    );
    assert_eq!(original.list_keys().await.unwrap().len(), 1);
    assert_eq!(
        restored.host_id().await.unwrap(),
        original.host_id().await.unwrap()
    );
    restored.close().await;
    original.close().await;
}

#[tokio::test]
async fn never_replaces_an_existing_destination() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let current = populated(&target).await;
    current.create_key("newer-client").await.unwrap();
    current.close().await;
    let source = root.path().join("legacy");
    populated(&source).await.close().await;
    assert!(
        recover_legacy(&target.join("app.db"), &[source])
            .await
            .unwrap()
            .is_none()
    );
    let current = Store::open(&target.join("app.db")).await.unwrap();
    assert_eq!(current.list_keys().await.unwrap().len(), 2);
    current.close().await;
}

#[tokio::test]
async fn conflicting_sources_fail_without_creating_an_empty_destination() {
    let root = tempfile::tempdir().unwrap();
    let one = root.path().join("one");
    let two = root.path().join("two");
    populated(&one).await.close().await;
    populated(&two).await.close().await;
    let target = root.path().join("app.db");
    assert!(recover_legacy(&target, &[one, two]).await.is_err());
    assert!(!target.exists());
}

#[tokio::test]
async fn corrupt_or_in_use_source_is_not_treated_as_first_launch() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    populated(&source).await.close().await;
    let target = root.path().join("app.db");
    let lock = InstanceGuard::acquire(&source.join("instance.lock")).unwrap();
    assert!(
        recover_legacy(&target, std::slice::from_ref(&source))
            .await
            .is_err()
    );
    assert!(!target.exists());
    drop(lock);
    let corrupt = root.path().join("corrupt");
    std::fs::create_dir(&corrupt).unwrap();
    std::fs::write(corrupt.join("app.db"), b"invalid database").unwrap();
    assert!(recover_legacy(&target, &[corrupt]).await.is_err());
    assert!(!target.exists());
}
