use tiny_proxy::storage::{FailureRecord, RequestRecord, Store};

fn record(id: &str, started_at: i64) -> RequestRecord {
    RequestRecord::new(id.into(), "POST".into(), "/v1/responses".into(), started_at)
}

#[tokio::test]
async fn request_and_failure_records_persist_separately_and_completion_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    let mut success = record("ok", 1000);
    success.model = "fixture".into();
    store.start_request(&success).await.unwrap();
    let mut failure = record("bad", 2000);
    store.start_request(&failure).await.unwrap();
    assert_eq!(store.request_records(0, 100).await.unwrap().len(), 2);
    assert!(store.failure_records(0, 100).await.unwrap().is_empty());
    success.outcome = "completed".into();
    success.ended_at = Some(1500);
    success.elapsed_ms = 500;
    store.finish_request(&success, None).await.unwrap();
    failure.outcome = "failed".into();
    failure.phase = "waiting_upstream".into();
    failure.ended_at = Some(2300);
    failure.elapsed_ms = 300;
    let error = FailureRecord {
        request_id: "bad".into(),
        occurred_at: 2300,
        phase: "waiting_upstream".into(),
        code: "upstream_timeout".into(),
        message: "Upstream request timed out".into(),
        http_status: 504,
    };
    store.finish_request(&failure, Some(&error)).await.unwrap();
    store.finish_request(&failure, Some(&error)).await.unwrap();
    store.close().await;
    let store = Store::open(&path).await.unwrap();
    let history = store.request_records(0, 100).await.unwrap();
    assert_eq!(history[0].id, "bad");
    assert_eq!(history[1].ended_at, Some(1500));
    assert_eq!(store.failure_records(0, 100).await.unwrap().len(), 1);
    assert_eq!(
        store
            .request_record("bad")
            .await
            .unwrap()
            .unwrap()
            .elapsed_ms,
        300
    );
    assert_eq!(store.record_counts().await.unwrap(), (2, 1));
}

#[tokio::test]
async fn recovery_closes_unfinished_requests_without_reclassifying_completed_ones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    let mut r = record("running", 1000);
    r.phase = "receiving".into();
    r.last_activity_at = Some(1200);
    r.elapsed_ms = 250;
    store.start_request(&r).await.unwrap();
    store.close().await;
    let store = Store::open(&path).await.unwrap();
    store
        .interrupt_requests(2000, "application_interrupted")
        .await
        .unwrap();
    store
        .interrupt_requests(3000, "application_interrupted")
        .await
        .unwrap();
    let r = store.request_record("running").await.unwrap().unwrap();
    assert_eq!(r.outcome, "interrupted");
    assert_eq!(r.ended_at, Some(2000));
    assert_eq!(r.elapsed_ms, 250, "closed-app time is not request duration");
    assert_eq!(r.last_activity_at, Some(1200));
    assert_eq!(store.failure_records(0, 100).await.unwrap().len(), 1);
    assert_eq!(
        store.failure_records(0, 100).await.unwrap()[0].phase,
        "receiving"
    );
}

#[tokio::test]
async fn retention_cleans_associated_failures_but_keeps_running_requests() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    for (id, start) in [("old", 1000), ("new", 2000), ("running", 500)] {
        let mut r = record(id, start);
        store.start_request(&r).await.unwrap();
        if id != "running" {
            r.ended_at = Some(start + 10);
            r.outcome = "failed".into();
            let error = FailureRecord {
                request_id: id.into(),
                occurred_at: start + 10,
                phase: "validating".into(),
                code: "invalid_request".into(),
                message: "Invalid request".into(),
                http_status: 400,
            };
            store.finish_request(&r, Some(&error)).await.unwrap();
        }
    }
    store.prune_request_records(1500, 10000).await.unwrap();
    assert!(store.request_record("old").await.unwrap().is_none());
    assert!(
        store
            .request_record("running")
            .await
            .unwrap()
            .unwrap()
            .ended_at
            .is_none()
    );
    assert_eq!(store.record_counts().await.unwrap(), (2, 1));
    store.prune_request_records(0, 0).await.unwrap();
    assert_eq!(store.record_counts().await.unwrap(), (1, 0));
}
