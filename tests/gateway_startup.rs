use std::{net::TcpListener, path::Path, time::Duration};
use tiny_proxy::{
    app::{Backend, Command, Event},
    auth::codex::now,
    storage::Store,
};
use tokio::sync::mpsc;

async fn next(events: &mut mpsc::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("backend did not respond within five seconds")
        .expect("backend closed unexpectedly")
}

async fn settle(events: &mut mpsc::Receiver<Event>) {
    loop {
        if matches!(
            next(events).await,
            Event::Status { busy: false, .. } | Event::Error(_)
        ) {
            return;
        }
    }
}

async fn running_address(events: &mut mpsc::Receiver<Event>) -> String {
    loop {
        match next(events).await {
            Event::Gateway {
                address: Some(address),
                ..
            } => return address,
            Event::Status {
                message,
                busy: false,
            } => panic!("gateway did not start: {message}"),
            Event::Error(message) => panic!("{message}"),
            _ => {}
        }
    }
}

async fn seed_ready(path: &Path) {
    let store = Store::open(path).await.unwrap();
    store
        .upsert_account("fixture", "Fixture", b"unused", now() + 3600, true)
        .await
        .unwrap();
    // No real upstream access is needed to test listener startup.
    store
        .save_usage("fixture", None, Some("fixture"), now())
        .await
        .unwrap();
    store.create_key("fixture-client").await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn upgrading_existing_settings_preserves_identity_and_defaults_to_loopback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let all = sqlx::migrate!("./migrations");
    let legacy = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter().filter(|m| m.version <= 3).cloned().collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO app_settings(singleton_id,host_id) VALUES(1,'fixture-host')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO codex_accounts(id,display_name,encrypted_blob,generation,expires_at) VALUES('fixture','Fixture',X'01',1,1)").execute(&pool).await.unwrap();
    sqlx::query("UPDATE app_settings SET active_codex_account_id='fixture'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.host_id().await.unwrap(), "fixture-host");
    assert_eq!(
        store.active_account().await.unwrap().as_deref(),
        Some("fixture")
    );
    assert_eq!(
        store.account_credentials("fixture").await.unwrap().unwrap(),
        (1, vec![1])
    );
    assert_eq!(store.gateway_bind().await.unwrap(), "127.0.0.1:8787");
    store.close().await;
}

async fn expect_error(events: &mut mpsc::Receiver<Event>, text: &str) {
    loop {
        match next(events).await {
            Event::Error(message) => {
                assert!(message.contains(text), "unexpected error: {message}");
                return;
            }
            Event::Gateway {
                address: Some(address),
                ..
            } => panic!("unexpected listener: {address}"),
            Event::Status {
                message,
                busy: false,
            } => panic!("error was hidden by ready: {message}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn missing_account_or_usable_key_keeps_the_app_ready_without_listening() {
    for (has_account, key_state, expected) in [
        (false, "valid", "设备码登录"),
        (true, "missing", "客户端密钥"),
        (true, "revoked", "客户端密钥"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        let store = Store::open(&path).await.unwrap();
        assert_eq!(store.gateway_bind().await.unwrap(), "127.0.0.1:8787");
        if has_account {
            store
                .upsert_account("fixture", "Fixture", b"unused", now() + 3600, true)
                .await
                .unwrap();
            store
                .save_usage("fixture", None, Some("fixture"), now())
                .await
                .unwrap();
        }
        if key_state != "missing" {
            let key = store.create_key("fixture-client").await.unwrap();
            if key_state == "revoked" {
                store.revoke_key(&key.id).await.unwrap();
            }
        }
        let mut backend = Backend::start(path).unwrap();
        let mut events = backend.events.take().unwrap();
        loop {
            match next(&mut events).await {
                Event::Status {
                    message,
                    busy: false,
                } => {
                    assert!(
                        message.contains(expected),
                        "missing setup instruction: {message}"
                    );
                    break;
                }
                Event::Error(message) => {
                    panic!("missing setup should not fail initialization: {message}")
                }
                Event::Gateway {
                    address: Some(_), ..
                } => panic!("gateway started without required configuration"),
                _ => {}
            }
        }
        backend
            .commands
            .send(Command::CreateKey("still-responsive".into()))
            .await
            .unwrap();
        loop {
            if matches!(next(&mut events).await, Event::KeyCreated(_)) {
                break;
            }
        }
        backend.stop();
        store.close().await;
    }
}

#[tokio::test]
async fn occupied_startup_address_reports_an_error_and_allows_manual_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    seed_ready(&path).await;
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupied.local_addr().unwrap();
    let store = Store::open(&path).await.unwrap();
    store.save_gateway_bind(address).await.unwrap();
    let mut backend = Backend::start(path).unwrap();
    let mut events = backend.events.take().unwrap();
    expect_error(&mut events, &format!("无法监听 {address}")).await;
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    backend
        .commands
        .send(Command::StartGateway(
            other.local_addr().unwrap().to_string(),
        ))
        .await
        .unwrap();
    expect_error(&mut events, "无法监听").await;
    assert_eq!(
        store.gateway_bind().await.unwrap(),
        address.to_string(),
        "failed bind replaced saved address"
    );
    drop(occupied);
    backend
        .commands
        .send(Command::StartGateway(address.to_string()))
        .await
        .unwrap();
    assert_eq!(running_address(&mut events).await, address.to_string());
    backend.stop();
    store.close().await;
}

#[tokio::test]
async fn manually_stopping_an_automatic_listener_keeps_it_stopped_until_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    seed_ready(&path).await;
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    let store = Store::open(&path).await.unwrap();
    store.save_gateway_bind(address).await.unwrap();
    drop(reservation);
    let mut backend = Backend::start(path.clone()).unwrap();
    let mut events = backend.events.take().unwrap();
    assert_eq!(running_address(&mut events).await, address.to_string());
    settle(&mut events).await;
    backend.commands.send(Command::StopGateway).await.unwrap();
    loop {
        if matches!(
            next(&mut events).await,
            Event::Gateway { address: None, .. }
        ) {
            break;
        }
    }
    settle(&mut events).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    while let Ok(event) = events.try_recv() {
        assert!(
            !matches!(
                event,
                Event::Gateway {
                    address: Some(_),
                    ..
                } | Event::Error(_)
            ),
            "stopped gateway was restarted by a background tick"
        );
    }
    let reservation = TcpListener::bind(address).expect("manual stop left the listener open");
    backend.stop();
    drop(reservation);
    let mut backend = Backend::start(path).unwrap();
    let mut events = backend.events.take().unwrap();
    assert_eq!(running_address(&mut events).await, address.to_string());
    backend.stop();
    store.close().await;
}

#[tokio::test]
async fn unsafe_saved_addresses_are_rejected_during_automatic_startup() {
    for value in [
        "0.0.0.0:8787",
        "8.8.8.8:8787",
        "127.0.0.1:0",
        "not-an-address",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        seed_ready(&path).await;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
        sqlx::query("UPDATE app_settings SET gateway_bind=?")
            .bind(value)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let mut backend = Backend::start(path).unwrap();
        let mut events = backend.events.take().unwrap();
        expect_error(&mut events, "网关自动启动失败").await;
        backend.stop();
    }
}

#[tokio::test]
async fn restarting_the_app_automatically_starts_the_last_successful_listener() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    seed_ready(&path).await;
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap().to_string();
    let mut backend = Backend::start(path.clone()).unwrap();
    let mut events = backend.events.take().unwrap();
    settle(&mut events).await;
    backend.commands.send(Command::StopGateway).await.unwrap();
    settle(&mut events).await;
    drop(reservation);
    backend
        .commands
        .send(Command::StartGateway(address.clone()))
        .await
        .unwrap();
    assert_eq!(running_address(&mut events).await, address);
    backend.stop();

    let mut backend = Backend::start(path).unwrap();
    let mut events = backend.events.take().unwrap();
    assert_eq!(running_address(&mut events).await, address);
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    backend.stop();
    assert!(
        TcpListener::bind(&address).is_ok(),
        "shutdown left the listener open"
    );
}
