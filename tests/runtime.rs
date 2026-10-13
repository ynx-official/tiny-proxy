use tiny_proxy::app::{Backend, Command, Event};

#[test]
fn background_runtime_creates_key_and_shuts_down_without_gpui() {
    let temp = tempfile::tempdir().unwrap();
    let mut backend = Backend::start(temp.path().join("app.db")).unwrap();
    let mut events = backend.events.take().unwrap();
    backend
        .commands
        .blocking_send(Command::CreateKey("test-client".into()))
        .unwrap();
    let started = std::time::Instant::now();
    let mut received = false;
    while started.elapsed() < std::time::Duration::from_secs(10) {
        match events.try_recv() {
            Ok(Event::KeyCreated(key)) => {
                assert!(key.value.starts_with("tp_"));
                received = true;
                break;
            }
            Ok(Event::Error(message)) => panic!("{message}"),
            _ => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
    assert!(received, "runtime did not respond within 10 seconds");
    drop(events);
    backend.stop();
}

#[test]
fn shutdown_does_not_depend_on_ui_draining_a_full_event_queue() {
    let temp = tempfile::tempdir().unwrap();
    let mut backend = Backend::start(temp.path().join("app.db")).unwrap();
    let _undrained_events = backend.events.take().unwrap();
    for index in 0..48 {
        let _ = backend
            .commands
            .try_send(Command::CreateKey(format!("test-{index}")));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        backend.stop();
        let _ = done.send(());
    });
    assert!(
        result
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_ok(),
        "shutdown waited for a stalled UI consumer"
    );
}

#[tokio::test]
async fn failed_startup_snapshot_is_not_overwritten_by_ready() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = tiny_proxy::storage::Store::open(&path).await.unwrap();
    store
        .upsert_account("one", "One", b"encrypted", 100, true)
        .await
        .unwrap();
    store.save_models("one", "invalid JSON", 100).await.unwrap();
    store.close().await;
    let mut backend = Backend::start(path).unwrap();
    let mut events = backend.events.take().unwrap();
    assert!(matches!(events.recv().await, Some(Event::Error(_))));
    assert!(
        events.recv().await.is_none(),
        "failed local state must not be reported as ready"
    );
    backend.stop();
}
