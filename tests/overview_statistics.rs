use chrono::{DateTime, Duration};
use tiny_proxy::{
    app::{Backend, Command, Event},
    overview::{OverviewPeriod, OverviewWindow},
    storage::{RequestRecord, Store},
    updater::{CheckMode, Preferences},
};

fn window() -> OverviewWindow {
    OverviewWindow::at(
        OverviewPeriod::Last24Hours,
        DateTime::parse_from_rfc3339("2026-10-14T16:30:00+08:00").unwrap(),
    )
    .unwrap()
}

fn record(id: &str, at: i64, outcome: &str) -> RequestRecord {
    let mut record = RequestRecord::new(id.into(), "POST".into(), "/v1/responses".into(), at);
    record.outcome = outcome.into();
    if outcome != "running" {
        record.ended_at = Some(at + 2_000);
        record.phase = "finished".into();
    }
    record
}

async fn persist(store: &Store, mut record: RequestRecord) {
    let ended = record.ended_at.take();
    store.start_request(&record).await.unwrap();
    record.ended_at = ended;
    if ended.is_some() {
        store.finish_request(&record, None).await.unwrap();
    }
}

#[test]
fn periods_use_local_midnight_and_exact_rolling_intervals() {
    let now = DateTime::parse_from_rfc3339("2026-10-14T16:30:00+08:00").unwrap();
    let today = OverviewWindow::at(OverviewPeriod::Today, now).unwrap();
    assert_eq!(
        today.start_ms,
        DateTime::parse_from_rfc3339("2026-10-14T00:00:00+08:00")
            .unwrap()
            .timestamp_millis()
    );
    assert_eq!(
        today.chart_end_ms - today.start_ms,
        Duration::days(1).num_milliseconds()
    );
    assert_eq!(
        OverviewWindow::at(OverviewPeriod::Last24Hours, now)
            .unwrap()
            .end_ms
            - window().start_ms,
        Duration::hours(24).num_milliseconds()
    );
    let week = OverviewWindow::at(OverviewPeriod::Last7Days, now).unwrap();
    assert_eq!(
        week.end_ms - week.start_ms,
        Duration::days(7).num_milliseconds()
    );
    assert_eq!(week.bucket_ms, Duration::hours(6).num_milliseconds());
}

#[tokio::test]
async fn query_covers_all_records_not_only_a_loaded_page_and_respects_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let window = window();
    for index in 0..151 {
        persist(
            &store,
            record(
                &format!("call-{index}"),
                window.start_ms + index,
                "completed",
            ),
        )
        .await;
    }
    persist(&store, record("before", window.start_ms - 1, "completed")).await;
    persist(&store, record("end-exclusive", window.end_ms, "completed")).await;
    let mut catalog = record("catalog", window.start_ms + 1, "completed");
    catalog.method = "GET".into();
    catalog.route = "/v1/models".into();
    persist(&store, catalog).await;
    let snapshot = store.overview(window, &[]).await.unwrap();
    assert_eq!(snapshot.totals.requests, 151);
    assert_eq!(snapshot.buckets.iter().map(|b| b.total()).sum::<i64>(), 151);
    assert_eq!(snapshot.recent.len(), 4);
    assert_eq!(snapshot.recent[0].id, "call-150");
    store.close().await;
}

#[tokio::test]
async fn outcomes_are_per_request_and_http_200_does_not_mean_stream_success() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let window = window();
    for (index, outcome) in [
        "completed",
        "completed",
        "completed",
        "failed",
        "incomplete",
        "interrupted",
        "cancelled",
        "running",
    ]
    .into_iter()
    .enumerate()
    {
        let mut item = record(
            &format!("call-{index}"),
            window.start_ms + index as i64,
            outcome,
        );
        item.http_status = Some(200);
        persist(&store, item).await;
    }
    let snapshot = store.overview(window, &[]).await.unwrap();
    assert_eq!(snapshot.totals.requests, 8);
    assert_eq!(snapshot.totals.completed, 3);
    assert_eq!(snapshot.totals.failed, 3);
    assert_eq!(snapshot.totals.cancelled, 1);
    assert_eq!(snapshot.totals.running, 1);
    assert_eq!(snapshot.totals.success_rate(), Some(0.5));
    store.close().await;
}

#[tokio::test]
async fn usage_preserves_unknown_zero_and_partial_values_and_latency_uses_real_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let window = window();
    for (index, latency) in [100, 300, 1_000, 2_000].into_iter().enumerate() {
        let mut item = record(
            &format!("stream-{index}"),
            window.start_ms + 10_000 + index as i64,
            "completed",
        );
        item.stream = Some(true);
        item.first_output_at = Some(item.started_at + latency);
        item.input_tokens = Some(if index == 0 { 0 } else { 100 });
        item.output_tokens = Some(20);
        persist(&store, item).await;
    }
    let mut unknown = record("unknown", window.start_ms + 100_000, "failed");
    unknown.stream = Some(true);
    unknown.first_byte_at = Some(unknown.started_at + 1);
    unknown.input_tokens = Some(-1);
    persist(&store, unknown).await;
    let snapshot = store.overview(window, &[]).await.unwrap();
    assert_eq!(snapshot.totals.input_tokens, Some(300));
    assert_eq!(snapshot.totals.output_tokens, Some(80));
    assert_eq!(snapshot.totals.total_tokens(), Some(380));
    assert!(!snapshot.totals.usage_complete());
    assert_eq!(snapshot.totals.usage_samples, 4);
    assert_eq!(snapshot.totals.latency_samples, 4);
    assert_eq!(snapshot.totals.p50_ms, Some(300));
    assert_eq!(snapshot.totals.p95_ms, Some(2_000));
    store.close().await;
}

#[tokio::test]
async fn live_overlay_is_deduplicated_and_cannot_reopen_a_committed_completion() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let window = window();
    let at = window.start_ms + 1;
    persist(&store, record("finished", at, "completed")).await;
    persist(&store, record("active", at + 1, "running")).await;
    let mut active = record("active", at + 1, "running");
    active.model = "fixture-live".into();
    let live = vec![
        record("finished", at, "running"),
        active,
        record("not-yet-flushed", at + 2, "running"),
    ];
    let snapshot = store.overview(window, &live).await.unwrap();
    assert_eq!(snapshot.totals.requests, 3);
    assert_eq!(snapshot.totals.completed, 1);
    assert_eq!(snapshot.totals.running, 2);
    assert!(
        snapshot
            .recent
            .iter()
            .any(|r| r.id == "active" && r.model == "fixture-live")
    );
    assert!(
        snapshot
            .recent
            .iter()
            .any(|r| r.id == "finished" && r.ended_at.is_some())
    );
    store.close().await;
}

#[tokio::test]
async fn empty_and_pruned_history_never_fabricates_totals_or_latency() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let window = window();
    let empty = store.overview(window.clone(), &[]).await.unwrap();
    assert_eq!(empty.totals.requests, 0);
    assert_eq!(empty.totals.success_rate(), None);
    assert_eq!(empty.totals.p50_ms, None);
    assert_eq!(empty.totals.total_tokens(), Some(0));
    persist(&store, record("retained", window.start_ms + 10, "failed")).await;
    persist(&store, record("removed", window.start_ms + 1, "completed")).await;
    store
        .prune_request_records(window.start_ms + 5, 10_000)
        .await
        .unwrap();
    let snapshot = store.overview(window, &[]).await.unwrap();
    assert_eq!(snapshot.totals.requests, 1);
    assert_eq!(snapshot.totals.total_tokens(), None);
    assert_eq!(snapshot.totals.success_rate(), Some(0.));
    store.close().await;
}

#[test]
fn today_keeps_future_hours_empty_and_counts_both_generation_routes() {
    let now = DateTime::parse_from_rfc3339("2026-10-14T16:30:00+08:00").unwrap();
    let window = OverviewWindow::at(OverviewPeriod::Today, now).unwrap();
    let first = record("responses", window.start_ms, "completed");
    let mut chat = record("chat", window.start_ms + 3_600_000, "completed");
    chat.route = "/v1/chat/completions".into();
    let mut other = record("other", window.start_ms + 1, "completed");
    other.route = "/v1/other".into();
    let future = record("future", window.end_ms, "completed");
    let snapshot = tiny_proxy::overview::aggregate(window, vec![first, chat, other, future], &[]);
    assert_eq!(snapshot.totals.requests, 2);
    assert_eq!(snapshot.buckets.len(), 24);
    assert_eq!(snapshot.buckets[0].completed, 1);
    assert_eq!(snapshot.buckets[1].completed, 1);
    assert!(snapshot.buckets.iter().skip(17).all(|b| b.total() == 0));
}

#[test]
fn observed_zero_latency_is_valid_but_bytes_and_non_streaming_outputs_are_not_samples() {
    let window = window();
    let at = window.start_ms + 10_000;
    let mut zero = record("zero", at, "completed");
    zero.stream = Some(true);
    zero.first_output_at = Some(at);
    zero.input_tokens = Some(0);
    zero.output_tokens = Some(0);
    let known = tiny_proxy::overview::aggregate(window.clone(), vec![zero.clone()], &[]);
    assert_eq!(known.totals.total_tokens(), Some(0));
    assert!(known.totals.usage_complete());
    let mut non_stream = record("non-stream", at, "completed");
    non_stream.stream = Some(false);
    non_stream.first_output_at = Some(at + 3);
    let mut missing = record("bytes-only", at, "failed");
    missing.stream = Some(true);
    missing.first_byte_at = Some(at + 1);
    let mut invalid = record("invalid-time", at, "failed");
    invalid.stream = Some(true);
    invalid.first_output_at = Some(at - 1);
    let snapshot =
        tiny_proxy::overview::aggregate(window, vec![zero, non_stream, missing, invalid], &[]);
    assert_eq!(snapshot.totals.latency_samples, 1);
    assert_eq!(snapshot.totals.p50_ms, Some(0));
    assert_eq!(snapshot.totals.p95_ms, Some(0));
    assert!(!snapshot.totals.usage_complete());
}

#[tokio::test]
async fn backend_returns_correlated_periods_and_refresh_reads_new_local_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let store = Store::open(&path).await.unwrap();
    store
        .save_update_preferences(&Preferences {
            mode: CheckMode::Disabled,
            ..Default::default()
        })
        .await
        .unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    persist(&store, record("recent", now - 60_000, "completed")).await;
    persist(
        &store,
        record(
            "older",
            now - Duration::days(2).num_milliseconds(),
            "failed",
        ),
    )
    .await;
    let mut backend = Backend::start(path).unwrap();
    let mut events = backend.events.take().unwrap();
    for (request_id, period, count) in [
        (41, OverviewPeriod::Last24Hours, 1),
        (42, OverviewPeriod::Last7Days, 2),
        (43, OverviewPeriod::Last24Hours, 2),
    ] {
        if request_id == 43 {
            persist(&store, record("new-on-refresh", now - 1_000, "completed")).await;
        }
        backend
            .commands
            .send(Command::LoadOverview { request_id, period })
            .await
            .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                match events.recv().await.expect("backend closed") {
                    Event::Overview {
                        request_id: received_id,
                        period: received_period,
                        result,
                    } => {
                        assert_eq!(received_id, request_id);
                        assert_eq!(received_period, period);
                        break result;
                    }
                    Event::Gateway {
                        address: Some(address),
                    } => panic!("unexpected listener: {address}"),
                    Event::Error(error) => panic!("unexpected backend failure: {error}"),
                    _ => {}
                }
            }
        })
        .await
        .expect("overview response timed out");
        let snapshot = result.unwrap();
        assert_eq!(snapshot.window.period, period);
        assert_eq!(snapshot.totals.requests, count);
    }
    backend.stop();
    store.close().await;
}
