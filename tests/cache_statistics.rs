use chrono::DateTime;
use tiny_proxy::{
    overview::{OverviewPeriod, OverviewWindow, aggregate},
    storage::{RequestRecord, Store},
};

fn window() -> OverviewWindow {
    OverviewWindow::at(
        OverviewPeriod::Last24Hours,
        DateTime::parse_from_rfc3339("2026-10-14T16:30:00+08:00").unwrap(),
    )
    .unwrap()
}

fn request(id: &str, input: Option<i64>, cached: Option<i64>) -> RequestRecord {
    let mut record = RequestRecord::new(
        id.into(),
        "POST".into(),
        "/v1/responses".into(),
        window().start_ms + 1,
    );
    record.input_tokens = input;
    record.cached_input_tokens = cached;
    record.outcome = "completed".into();
    record.ended_at = Some(record.started_at + 10);
    record
}

#[test]
fn cache_rate_is_token_weighted_over_valid_paired_samples_and_never_inferred_from_success() {
    let snapshot = aggregate(
        window(),
        vec![
            request("large-hit", Some(1_000), Some(900)),
            request("explicit-miss", Some(100), Some(0)),
            request("legacy-missing", Some(100), None),
            request("over-input", Some(200), Some(201)),
            request("negative", Some(100), Some(-1)),
            request("unknown-input", None, Some(10)),
            request("zero-input", Some(0), Some(0)),
        ],
        &[],
    );
    assert_eq!(snapshot.totals.requests, 7);
    assert_eq!(snapshot.totals.success_rate(), Some(1.));
    assert_eq!(snapshot.totals.cache_samples, 3);
    assert_eq!(snapshot.totals.cached_input_tokens, Some(900));
    assert_eq!(snapshot.totals.cache_input_tokens, Some(1_100));
    assert_eq!(
        snapshot.totals.total_tokens(),
        Some(1_500),
        "cache is already part of input usage"
    );
    let rate = snapshot.totals.cache_hit_rate().unwrap();
    assert!((rate - 900. / 1_100.).abs() < 1e-12);
    assert!(!snapshot.totals.cache_usage_complete());
}

#[test]
fn unknown_zero_full_and_overflowing_cache_values_have_distinct_meanings() {
    for (records, expected, samples) in [
        (vec![], None, 0),
        (vec![request("unknown", Some(100), None)], None, 0),
        (vec![request("zero-input", Some(0), Some(0))], None, 1),
        (vec![request("miss", Some(100), Some(0))], Some(0.), 1),
        (vec![request("full", Some(100), Some(100))], Some(1.), 1),
        (
            vec![
                request("one", Some(i64::MAX), Some(i64::MAX)),
                request("two", Some(1), Some(1)),
            ],
            None,
            2,
        ),
    ] {
        let snapshot = aggregate(window(), records, &[]);
        assert_eq!(snapshot.totals.cache_hit_rate(), expected);
        assert_eq!(snapshot.totals.cache_samples, samples);
    }
}

#[test]
fn cache_samples_follow_the_same_period_filter_and_live_terminal_deduplication() {
    let window = window();
    let saved = request("done", Some(100), Some(80));
    let mut stale_live = request("done", Some(100), Some(1));
    stale_live.ended_at = None;
    let mut active = request("active", Some(50), Some(25));
    active.ended_at = None;
    let mut excluded = request("excluded", Some(10_000), Some(10_000));
    excluded.started_at = window.start_ms - 1;
    let snapshot = aggregate(window, vec![saved, excluded], &[stale_live, active]);
    assert_eq!(snapshot.totals.requests, 2);
    assert_eq!(snapshot.totals.cache_samples, 2);
    assert_eq!(snapshot.totals.cached_input_tokens, Some(105));
    assert_eq!(snapshot.totals.cache_input_tokens, Some(150));
    assert_eq!(snapshot.totals.cache_hit_rate(), Some(0.7));
}

#[tokio::test]
async fn legacy_cache_stays_unknown_and_terminal_values_survive_reopen() {
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
            all.iter().filter(|m| m.version <= 8).cloned().collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO request_records(id,method,route,outcome,phase,started_at,ended_at,input_tokens) VALUES('legacy','POST','/v1/responses','completed','finished',?,?,100)")
        .bind(window().start_ms + 1).bind(window().start_ms + 20).execute(&pool).await.unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    let legacy = store.request_record("legacy").await.unwrap().unwrap();
    assert_eq!(legacy.input_tokens, Some(100));
    assert_eq!(legacy.cached_input_tokens, None);
    let mut current = request("current", Some(200), Some(100));
    current.ended_at = None;
    store.start_request(&current).await.unwrap();
    current.cached_input_tokens = Some(120);
    store.checkpoint_request(&current).await.unwrap();
    assert_eq!(
        store
            .request_record("current")
            .await
            .unwrap()
            .unwrap()
            .cached_input_tokens,
        Some(120)
    );
    current.cached_input_tokens = Some(150);
    current.ended_at = Some(current.started_at + 20);
    store.finish_request(&current, None).await.unwrap();
    current.cached_input_tokens = Some(199);
    store.checkpoint_request(&current).await.unwrap();
    store.close().await;

    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store
            .request_record("current")
            .await
            .unwrap()
            .unwrap()
            .cached_input_tokens,
        Some(150)
    );
    assert_eq!(
        store
            .request_record("legacy")
            .await
            .unwrap()
            .unwrap()
            .cached_input_tokens,
        None
    );
    let snapshot = store.overview(window(), &[]).await.unwrap();
    assert_eq!(snapshot.totals.requests, 2);
    assert_eq!(snapshot.totals.cache_samples, 1);
    assert_eq!(snapshot.totals.cache_hit_rate(), Some(0.75));
    store.close().await;
}
