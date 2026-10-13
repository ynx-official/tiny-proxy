//! Catalog check using the desktop application's saved session.
//! --test-text explicitly opts into one fixed short generation on the first model.
//! Prints only counts and sanitized errors, not credentials or response bodies.
use tiny_proxy::{auth::codex::Service, storage::Store};

#[tokio::main]
async fn main() {
    let Ok(location) = tiny_proxy::platform::data_dir::DataLocation::from_environment() else {
        eprintln!("data_directory_unavailable");
        std::process::exit(1);
    };
    let data = location.directory;
    // Initialize/migrate in the desktop first, then probe the same database.
    if !data.join("app.db").is_file() {
        eprintln!("start_desktop_first");
        std::process::exit(1);
    }
    let Ok(store) = Store::open(&data.join("app.db")).await else {
        eprintln!("storage_unavailable");
        std::process::exit(1);
    };
    let result = match Service::new(store.clone()) {
        Ok(service) => match service.models().await {
            Ok(models) => {
                println!("model_catalog_ok count={}", models.len());
                if std::env::args().any(|arg| arg == "--refresh-usage") {
                    let id = service.active_id().await.expect("active account");
                    if let Err(error) = service.refresh_usage(&id).await {
                        eprintln!("quota_query_failed: {error}");
                    } else {
                        let account = store.account(&id).await.unwrap().unwrap();
                        let quota: tiny_proxy::auth::quota::Quota =
                            serde_json::from_str(account.usage_json.as_deref().unwrap()).unwrap();
                        println!(
                            "quota_saved windows={} reset_count_available={} reset_query_error={}",
                            quota.windows.len(),
                            quota.reset_credits.is_some(),
                            quota.reset_error.is_some()
                        );
                    }
                }
                if std::env::args().any(|arg| arg == "--test-text") {
                    println!("text_probe_model={}", models[0].slug);
                    tiny_proxy::app::test_and_save(
                        &service,
                        &store,
                        &models[0].slug,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                    .map(|result| {
                        println!("text_probe_completed chars={}", result.text.chars().count())
                    })
                } else {
                    Ok(())
                }
            }
            Err(error) => Err(error.to_string()),
        },
        Err(error) => Err(error.to_string()),
    };
    store.close().await;
    match result {
        Ok(()) => {}
        Err(error) => {
            eprintln!("model_check_failed: {error}");
            std::process::exit(1);
        }
    }
}
