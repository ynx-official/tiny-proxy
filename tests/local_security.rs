use tiny_proxy::{
    platform::{protection, single_instance::InstanceGuard},
    storage::{Error, KeyStatus, Store},
};

#[cfg(windows)]
#[test]
fn dpapi_roundtrip_and_tamper_rejection() {
    let plaintext = b"synthetic-refresh-token-do-not-use";
    let mut ciphertext = protection::protect(plaintext).unwrap();
    assert!(
        !ciphertext
            .windows(plaintext.len())
            .any(|part| part == plaintext)
    );
    assert_eq!(&**protection::unprotect(&ciphertext).unwrap(), plaintext);
    let index = ciphertext.len() / 2;
    ciphertext[index] ^= 0x80;
    assert!(protection::unprotect(&ciphertext).is_err());
    assert!(protection::unprotect(&[]).is_err());
}

#[test]
fn process_lock_blocks_second_owner_and_releases_on_drop() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("instance.lock");
    let guard = InstanceGuard::acquire(&path).unwrap();
    assert!(InstanceGuard::acquire(&path).is_err());
    drop(guard);
    assert!(InstanceGuard::acquire(&path).is_ok());
}

#[tokio::test]
async fn host_identity_and_key_revocation_survive_restart_without_plaintext_secret() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("app.db");
    let db = Store::open(&path).await.unwrap();
    let host = db.host_id().await.unwrap();
    assert!(host.starts_with("urn:uuid:"));
    let key = db.create_key("工作电脑 Trae").await.unwrap();
    assert!(key.value.len() >= 75);
    assert_eq!(
        db.authenticate(&key.value).await.unwrap(),
        KeyStatus::Valid(key.id.clone())
    );
    let mut tampered = key.value.to_string();
    tampered.push('x');
    assert_eq!(
        db.authenticate(&tampered).await.unwrap(),
        KeyStatus::Invalid
    );
    assert_eq!(
        db.authenticate("not-a-key").await.unwrap(),
        KeyStatus::Invalid
    );
    db.close().await;
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes
            .windows(key.value.len())
            .any(|part| part == key.value.as_bytes())
    );
    let secret = key.value.split_once('.').unwrap().1.as_bytes();
    assert!(!bytes.windows(secret.len()).any(|part| part == secret));
    let db = Store::open(&path).await.unwrap();
    assert_eq!(db.host_id().await.unwrap(), host);
    assert_eq!(db.list_keys().await.unwrap().len(), 1);
    db.revoke_key(&key.id).await.unwrap();
    assert_eq!(
        db.authenticate(&key.value).await.unwrap(),
        KeyStatus::Invalid
    );
    assert_eq!(
        db.authenticate(&tampered).await.unwrap(),
        KeyStatus::Invalid
    );
    assert!(db.list_keys().await.unwrap().is_empty());
    assert!(matches!(
        db.reveal_key(&key.id).await,
        Err(Error::KeyNotFound)
    ));
    assert!(matches!(
        db.create_key("   ").await,
        Err(Error::InvalidLabel)
    ));
    db.close().await;
}

#[tokio::test]
async fn issued_registration_is_retained_and_rotations_compare_generation() {
    let temp = tempfile::tempdir().unwrap();
    let db = Store::open(&temp.path().join("app.db")).await.unwrap();
    let id = db
        .retain_registration("oaiapp_synthetic_client")
        .await
        .unwrap();
    assert!(!id.is_empty());
    assert_eq!(
        id,
        db.retain_registration("oaiapp_synthetic_client")
            .await
            .unwrap()
    );
    assert!(
        db.retain_registration("dynamic_agent_client")
            .await
            .is_err()
    );
    assert_eq!(
        db.save_credentials(&id, 0, b"synthetic-ciphertext-v1", 100)
            .await
            .unwrap(),
        1
    );
    let (a, b) = tokio::join!(
        db.save_credentials(&id, 1, b"synthetic-ciphertext-v2a", 200),
        db.save_credentials(&id, 1, b"synthetic-ciphertext-v2b", 200)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(a, Err(Error::Conflict)) || matches!(b, Err(Error::Conflict)));
    let (generation, blob) = db.load_credentials(&id).await.unwrap().unwrap();
    assert_eq!(generation, 2);
    assert!(blob == b"synthetic-ciphertext-v2a" || blob == b"synthetic-ciphertext-v2b");
    db.close().await;
}

#[tokio::test]
async fn sign_in_binds_identity_and_activation_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let db = Store::open(&temp.path().join("app.db")).await.unwrap();
    let id = db
        .retain_registration("oaiapp_synthetic_client")
        .await
        .unwrap();
    db.complete_sign_in(&id, "subject-one", true, b"cipher-one", 123)
        .await
        .unwrap();
    assert!(db.registrations().await.unwrap()[0].active);
    assert!(
        db.complete_sign_in(&id, "subject-two", true, b"cipher-two", 456)
            .await
            .is_err()
    );
    assert_eq!(
        db.load_credentials(&id).await.unwrap().unwrap().1,
        b"cipher-one"
    );
    assert_eq!(
        db.registrations().await.unwrap()[0].permission_state,
        "connected"
    );
    db.close().await;
}
