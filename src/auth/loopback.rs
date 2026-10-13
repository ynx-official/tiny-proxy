use super::attempt::{Attempt, CodeGrant};
use axum::{
    Router,
    extract::{RawQuery, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
#[error("login callback listener failed or was cancelled")]
pub struct Error;
struct CallbackState {
    attempt: Attempt,
    sender: Option<oneshot::Sender<Result<CodeGrant, Error>>>,
}
pub struct Listener {
    pub authorization_url: url::Url,
    pub port: u16,
    receiver: Option<oneshot::Receiver<Result<CodeGrant, Error>>>,
    server: Option<JoinHandle<()>>,
    shutdown: CancellationToken,
}
impl Listener {
    pub async fn bind(host_id: &str, client_id: Option<&str>) -> Result<Self, Error> {
        let tcp = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|_| Error)?;
        let port = tcp.local_addr().map_err(|_| Error)?.port();
        let attempt = Attempt::new(port, host_id, client_id, Instant::now());
        let authorization_url = attempt.authorization_url();
        let (sender, receiver) = oneshot::channel();
        let state = Arc::new(Mutex::new(CallbackState {
            attempt,
            sender: Some(sender),
        }));
        let app = Router::new()
            .route("/auth/callback", get(callback))
            .with_state(state);
        let shutdown = CancellationToken::new();
        let stop = shutdown.clone();
        let server = tokio::spawn(async move {
            let _ = axum::serve(tcp, app)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await;
        });
        Ok(Self {
            authorization_url,
            port,
            receiver: Some(receiver),
            server: Some(server),
            shutdown,
        })
    }
    pub async fn wait(mut self, cancel: CancellationToken) -> Result<CodeGrant, Error> {
        let receiver = self.receiver.take().ok_or(Error)?;
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(Error),
            result = receiver => result.unwrap_or(Err(Error)),
            _ = tokio::time::sleep(Duration::from_secs(600)) => Err(Error),
        };
        self.shutdown.cancel();
        if let Some(mut server) = self.server.take()
            && tokio::time::timeout(Duration::from_secs(1), &mut server)
                .await
                .is_err()
        {
            server.abort();
            let _ = server.await;
        }
        result
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(server) = &self.server {
            server.abort();
        }
    }
}

async fn callback(
    State(state): State<Arc<Mutex<CallbackState>>>,
    RawQuery(query): RawQuery,
) -> Response {
    let mut state = state.lock().await;
    let result = state
        .attempt
        .callback(query.as_deref().unwrap_or(""), Instant::now());
    let accepted = result.is_ok();
    match result {
        Ok(grant) => {
            if let Some(sender) = state.sender.take() {
                let _ = sender.send(Ok(grant));
            }
        }
        Err(super::attempt::Error::InvalidCallback) => {}
        Err(_) => {
            if let Some(sender) = state.sender.take() {
                let _ = sender.send(Err(Error));
            }
        }
    }
    // Entirely static: never reflect query parameters, codes, identities or errors.
    let html = if accepted {
        "<!doctype html><meta charset=utf-8><title>Tiny Proxy</title><script>history.replaceState(null,'','/auth/callback')</script><h1>已收到授权回调</h1><p>请返回 Tiny Proxy 查看校验结果。此页面可以关闭。</p>"
    } else {
        "<!doctype html><meta charset=utf-8><title>Tiny Proxy</title><script>history.replaceState(null,'','/auth/callback')</script><h1>此登录回调不可用</h1><p>请返回 Tiny Proxy 重新开始登录。</p>"
    };
    (if accepted { StatusCode::OK } else { StatusCode::BAD_REQUEST }, [
        ("content-type", "text/html; charset=utf-8"),
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("content-security-policy", "default-src 'none'; script-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'"),
        ("x-content-type-options", "nosniff"),
    ], html).into_response()
}
