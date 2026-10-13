//! Codex device authorization and upstream transport. Endpoint/client contracts
//! follow the local cursor-byok reference; no credentials are read from Codex.
use crate::{platform::protection, storage::Store};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
const REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("请先完成 Codex 设备码登录")]
    NoAccount,
    #[error("Codex 登录已过期或被撤销，请重新进行设备码登录")]
    AuthRequired,
    #[error("Codex 授权已取消或过期，请重新获取设备码")]
    Cancelled,
    #[error("Codex 授权被拒绝；请检查账号是否启用设备码登录")]
    Denied,
    #[error("Codex 上游返回 HTTP {0}")]
    Http(u16),
    #[error("Codex {stage}失败（HTTP {status}），请稍后重试")]
    AuthHttp { stage: &'static str, status: u16 },
    #[error("连接 Codex 上游失败，请检查网络后重试")]
    Network,
    #[error("Codex 上游请求超时")]
    Timeout,
    #[error("Codex 返回的数据未通过协议校验")]
    Protocol,
    #[error("Codex 模型目录不是有效 JSON，请重新读取目录")]
    CatalogJson,
    #[error("Codex 模型目录缺少 models/data 数组")]
    CatalogShape,
    #[error("Codex 模型目录中没有可用模型")]
    CatalogEmpty,
    #[error("Codex 模型目录超过 1 MiB 大小限制")]
    CatalogTooLarge,
    #[error("Codex 模型调用未返回 SSE 流（响应类型：{0}）")]
    StreamType(&'static str),
    #[error("无法安全读取或保存 Codex 凭据")]
    Storage,
    #[error("该模型不在当前 Codex 账号的可用目录中")]
    Model,
}

pub struct DeviceSession {
    device_id: Zeroizing<String>,
    pub user_code: Zeroizing<String>,
    pub interval: Duration,
    pub expires_in: Duration,
}
fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn number(v: &Value, key: &str) -> Option<u64> {
    v.get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
}
impl DeviceSession {
    pub fn parse(v: &Value) -> Result<Self, Error> {
        let seconds = number(v, "expires_in").unwrap_or(900).min(900);
        let interval = number(v, "interval").unwrap_or(5).max(1);
        let id = text(v, "device_auth_id")
            .or_else(|| text(v, "device_code"))
            .ok_or(Error::Protocol)?;
        let code = text(v, "user_code")
            .or_else(|| text(v, "usercode"))
            .ok_or(Error::Protocol)?;
        if seconds == 0
            || interval > 900
            || id.len() > 4096
            || code.len() > 128
            || code.chars().any(char::is_control)
        {
            return Err(Error::Protocol);
        }
        Ok(Self {
            device_id: Zeroizing::new(id),
            user_code: Zeroizing::new(code),
            interval: Duration::from_secs(interval),
            expires_in: Duration::from_secs(seconds),
        })
    }
}
pub enum Poll {
    Pending,
    SlowDown,
    Code {
        code: Zeroizing<String>,
        verifier: Zeroizing<String>,
    },
    Tokens(Value),
}
pub fn parse_poll(status: u16, v: Value) -> Result<Poll, Error> {
    // Only this device endpoint uses 403/404 for pending authorization. Never
    // hide unrelated server failures or accept credentials from a failed reply.
    if !(200..300).contains(&status) && !matches!(status, 400 | 403 | 404) {
        return Err(Error::Http(status));
    }
    let code = v["error"]
        .as_str()
        .or_else(|| v["error"]["code"].as_str())
        .or_else(|| v["error"]["type"].as_str())
        .or_else(|| v["status"].as_str())
        .or_else(|| v["state"].as_str())
        .unwrap_or("");
    // Explicit denial/expiration wins over the endpoint's 403/404 pending convention.
    match code {
        "access_denied" | "denied" => return Err(Error::Denied),
        "expired_token" | "expired" => return Err(Error::Cancelled),
        "slow_down" => return Ok(Poll::SlowDown),
        "authorization_pending"
        | "pending"
        | "waiting"
        | "in_progress"
        | "device_authorization_pending"
        | "deviceauth_authorization_pending" => {
            return Ok(Poll::Pending);
        }
        _ => {}
    }
    if status == 403 || status == 404 {
        return Ok(Poll::Pending);
    }
    let message = v["error_description"]
        .as_str()
        .or_else(|| v["message"].as_str())
        .or_else(|| v["error"]["message"].as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if message.contains("authorization is pending") || message.contains("authorization_pending") {
        return Ok(Poll::Pending);
    }
    if !(200..300).contains(&status) {
        return Err(Error::Http(status));
    }
    if v.get("error").is_some_and(|error| !error.is_null()) {
        return Err(Error::Protocol);
    }
    if text(&v, "access_token").is_some() {
        return Ok(Poll::Tokens(v));
    }
    match (text(&v, "authorization_code"), text(&v, "code_verifier")) {
        (Some(code), Some(verifier)) => Ok(Poll::Code {
            code: Zeroizing::new(code),
            verifier: Zeroizing::new(verifier),
        }),
        _ => Err(Error::Protocol),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
}
fn catalog_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .find_map(|name| value.get(name).filter(|v| !v.is_null()))
}
fn catalog_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn catalog_id(value: &Value) -> Option<String> {
    catalog_text(value)
        .or_else(|| catalog_text(catalog_field(value, &["slug", "id", "model", "name"])?))
}
pub fn parse_models(value: Value) -> Result<Vec<Model>, Error> {
    let items = catalog_field(&value, &["models", "data"])
        .unwrap_or(&value)
        .as_array()
        .ok_or(Error::CatalogShape)?;
    let mut seen = HashSet::new();
    let mut result = vec![];
    for item in items {
        if !item.is_object()
            || item["supported_in_api"] == false
            || item["supportedInApi"] == false
            || catalog_text(&item["visibility"]).is_some_and(|s| s.eq_ignore_ascii_case("hidden"))
        {
            continue;
        }
        let Some(slug) = catalog_id(item) else {
            continue;
        };
        if slug.len() > 200 || slug.chars().any(char::is_control) || !seen.insert(slug.clone()) {
            continue;
        }
        let efforts = catalog_field(
            item,
            &[
                "supported_reasoning_levels",
                "supportedReasoningLevels",
                "reasoning_levels",
                "reasoningLevels",
                "supported_reasoning_efforts",
                "supportedReasoningEfforts",
                "reasoning_efforts",
                "reasoningEfforts",
            ],
        )
        .and_then(Value::as_array);
        let mut seen_efforts = HashSet::new();
        result.push(Model {
            display_name: catalog_field(item, &["display_name", "displayName", "title", "name"])
                .and_then(catalog_text)
                .unwrap_or_else(|| slug.clone()),
            slug,
            reasoning_efforts: efforts
                .into_iter()
                .flatten()
                .filter_map(|v| {
                    catalog_text(v).or_else(|| {
                        catalog_text(catalog_field(v, &["effort", "id", "value", "name"])?)
                    })
                })
                .filter(|effort| seen_efforts.insert(effort.clone()))
                .collect(),
        });
    }
    if result.is_empty() {
        return Err(Error::CatalogEmpty);
    }
    if let Some(default) = catalog_field(
        &value,
        &[
            "default_model",
            "defaultModel",
            "default_model_slug",
            "defaultModelSlug",
            "primary_model",
            "primaryModel",
        ],
    )
    .and_then(catalog_id)
    {
        result.sort_by_key(|m| m.slug != default);
    }
    Ok(result)
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct Bundle {
    access_token: String,
    refresh_token: String,
    account_id: String,
    subject: String,
    expires_at: i64,
}
impl Bundle {
    fn local_id(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.subject.as_bytes());
        hash.update([0]);
        hash.update(self.account_id.as_bytes());
        format!("{:x}", hash.finalize())
    }
    fn label(&self) -> String {
        // This is a display hint, not an independently verified identity claim.
        let email = self
            .access_token
            .split('.')
            .nth(1)
            .and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
            .and_then(|v| text(&v["https://api.openai.com/profile"], "email"));
        email
            .filter(|s| s.len() <= 160 && !s.chars().any(char::is_control))
            .unwrap_or_else(|| {
                format!(
                    "ChatGPT · {}",
                    self.account_id
                        .chars()
                        .rev()
                        .take(8)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                )
            })
    }
    fn from_reply(v: &Value, previous: Option<&Self>) -> Result<Self, Error> {
        let access = text(v, "access_token").ok_or(Error::Protocol)?;
        if access.len() > 64 * 1024 {
            return Err(Error::Protocol);
        }
        let refresh = text(v, "refresh_token")
            .or_else(|| previous.map(|b| b.refresh_token.clone()))
            .unwrap_or_default();
        // Claims are routing/expiry metadata from a token delivered by the fixed
        // HTTPS token endpoint, NOT a locally verified identity or entitlement.
        let payload = access.split('.').nth(1).ok_or(Error::Protocol)?;
        let raw = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(payload)
                .map_err(|_| Error::Protocol)?,
        );
        let claims: Value = serde_json::from_slice(&raw).map_err(|_| Error::Protocol)?;
        let account = text(&claims["https://api.openai.com/auth"], "chatgpt_account_id")
            .ok_or(Error::Protocol)?;
        let subject = text(&claims, "sub").ok_or(Error::Protocol)?;
        if account.len() > 256
            || !account
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error::Protocol);
        }
        if previous.is_some_and(|b| b.account_id != account || b.subject != subject) {
            return Err(Error::AuthRequired);
        }
        let exp = claims["exp"].as_i64().ok_or(Error::Protocol)?;
        let expires_at = number(v, "expires_in")
            .map(|s| now() + s.min(86400) as i64)
            .unwrap_or(exp)
            .min(exp);
        if expires_at <= now() {
            return Err(Error::AuthRequired);
        }
        Ok(Self {
            access_token: access,
            refresh_token: refresh,
            account_id: account,
            subject,
            expires_at,
        })
    }
}

pub struct Service {
    store: Store,
    http: reqwest::Client,
    auth_base: String,
    api_base: String,
    refresh_lock: Mutex<()>,
}
impl Service {
    pub fn new(store: Store) -> Result<Arc<Self>, Error> {
        Self::build(
            store,
            "https://auth.openai.com".into(),
            "https://chatgpt.com/backend-api/codex".into(),
        )
    }
    fn build(store: Store, auth_base: String, api_base: String) -> Result<Arc<Self>, Error> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .user_agent("tiny-proxy/0.1.0")
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Arc::new(Self {
            store,
            http,
            auth_base,
            api_base,
            refresh_lock: Mutex::new(()),
        }))
    }
    /// Local HTTP fixture only. Production construction has no endpoint override.
    #[doc(hidden)]
    pub fn for_test(store: Store, address: std::net::SocketAddr) -> Result<Arc<Self>, Error> {
        if !address.ip().is_loopback() {
            return Err(Error::Protocol);
        }
        let url = format!("http://{address}");
        Self::build(store, url.clone(), url)
    }
    pub async fn connected(&self) -> Result<bool, Error> {
        if self
            .store
            .active_account()
            .await
            .map_err(|_| Error::Storage)?
            .is_some()
        {
            return Ok(true);
        }
        self.initialize().await?;
        Ok(self
            .store
            .codex_credentials()
            .await
            .map_err(|_| Error::Storage)?
            .is_some())
    }
    pub async fn initialize(&self) -> Result<(), Error> {
        let _lock = self.refresh_lock.lock().await;
        self.migrate_legacy().await
    }
    async fn migrate_legacy(&self) -> Result<(), Error> {
        if !self
            .store
            .accounts()
            .await
            .map_err(|_| Error::Storage)?
            .is_empty()
        {
            return Ok(());
        }
        if let Some((_, blob)) = self
            .store
            .codex_credentials()
            .await
            .map_err(|_| Error::Storage)?
        {
            let raw = protection::unprotect(&blob).map_err(|_| Error::Storage)?;
            let bundle: Bundle = serde_json::from_slice(&raw).map_err(|_| Error::Storage)?;
            self.store
                .upsert_account(
                    &bundle.local_id(),
                    &bundle.label(),
                    &blob,
                    bundle.expires_at,
                    true,
                )
                .await
                .map_err(|_| Error::Storage)?;
        }
        Ok(())
    }
    pub async fn active_id(&self) -> Result<String, Error> {
        if let Some(id) = self
            .store
            .active_account()
            .await
            .map_err(|_| Error::Storage)?
        {
            return Ok(id);
        }
        self.initialize().await?;
        self.store
            .active_account()
            .await
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NoAccount)
    }
    pub async fn activate(&self, id: &str) -> Result<(), Error> {
        self.store
            .activate_account(id)
            .await
            .map_err(|_| Error::Storage)
    }
    pub async fn begin(&self) -> Result<DeviceSession, Error> {
        let response = self
            .http
            .post(format!(
                "{}/api/accounts/deviceauth/usercode",
                self.auth_base
            ))
            .timeout(Duration::from_secs(30))
            .header("accept", "application/json")
            .json(&json!({"client_id":CLIENT_ID}))
            .send()
            .await
            .map_err(network)?;
        DeviceSession::parse(&auth_json(response, "申请设备码").await?)
    }
    pub async fn finish_device(
        self: &Arc<Self>,
        session: DeviceSession,
        cancel: CancellationToken,
    ) -> Result<(), Error> {
        let work = async {
            let mut interval = session.interval;
            loop {
                tokio::time::sleep(interval).await;
                let response = self.http.post(format!("{}/api/accounts/deviceauth/token", self.auth_base))
                    .timeout(Duration::from_secs(30)).header("accept", "application/json").json(&json!({"device_auth_id":session.device_id.as_str(), "user_code": session.user_code.as_str()}))
                    .send().await.map_err(network)?;
                let reply = match device_poll(response).await? {
                    Poll::Pending => continue,
                    Poll::SlowDown => {
                        interval += Duration::from_secs(5);
                        continue;
                    }
                    Poll::Tokens(v) => v,
                    Poll::Code { code, verifier } => {
                        let response = self
                            .http
                            .post(format!("{}/oauth/token", self.auth_base))
                            .timeout(Duration::from_secs(30))
                            .header("accept", "application/json")
                            .form(&[
                                ("grant_type", "authorization_code"),
                                ("client_id", CLIENT_ID),
                                ("redirect_uri", REDIRECT_URI),
                                ("code", code.as_str()),
                                ("code_verifier", verifier.as_str()),
                            ])
                            .send()
                            .await
                            .map_err(network)?;
                        auth_json(response, "换取登录凭据").await?
                    }
                };
                let bundle = Bundle::from_reply(&reply, None)?;
                let _lock = self.refresh_lock.lock().await;
                self.migrate_legacy().await?;
                let bytes =
                    Zeroizing::new(serde_json::to_vec(&bundle).map_err(|_| Error::Storage)?);
                let blob = protection::protect(&bytes).map_err(|_| Error::Storage)?;
                self.store
                    .upsert_account(
                        &bundle.local_id(),
                        &bundle.label(),
                        &blob,
                        bundle.expires_at,
                        true,
                    )
                    .await
                    .map_err(|_| Error::Storage)?;
                return Ok(());
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(Error::Cancelled),
            result = tokio::time::timeout(session.expires_in, work) => result.unwrap_or(Err(Error::Cancelled)),
        }
    }
    async fn save(&self, id: &str, generation: i64, bundle: &Bundle) -> Result<(), Error> {
        let bytes = Zeroizing::new(serde_json::to_vec(bundle).map_err(|_| Error::Storage)?);
        let protected = protection::protect(&bytes).map_err(|_| Error::Storage)?;
        self.store
            .rotate_account(id, generation, &protected, bundle.expires_at)
            .await
            .map_err(|_| Error::Storage)
    }
    async fn credentials_for(self: &Arc<Self>, id: &str) -> Result<Bundle, Error> {
        let service = self.clone();
        let id = id.to_owned();
        // A disconnected HTTP client must not interrupt a refresh after the
        // upstream rotates its token. The owned task completes the atomic save.
        tokio::spawn(async move {
            let _lock = service.refresh_lock.lock().await;
            let (generation, bytes) = service
                .store
                .account_credentials(&id)
                .await
                .map_err(|_| Error::Storage)?
                .ok_or(Error::NoAccount)?;
            let plaintext = protection::unprotect(&bytes).map_err(|_| Error::Storage)?;
            let previous: Bundle =
                serde_json::from_slice(&plaintext).map_err(|_| Error::Storage)?;
            if previous.expires_at > now() + 60 {
                return Ok(previous);
            }
            if previous.refresh_token.is_empty() {
                return Err(Error::AuthRequired);
            }
            let response = service
                .http
                .post(format!("{}/oauth/token", service.auth_base))
                .timeout(Duration::from_secs(30))
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", CLIENT_ID),
                    ("refresh_token", previous.refresh_token.as_str()),
                ])
                .send()
                .await
                .map_err(network)?;
            if matches!(response.status().as_u16(), 400 | 401 | 403) {
                return Err(Error::AuthRequired);
            }
            let next = Bundle::from_reply(&checked_json(response).await?, Some(&previous))?;
            service.save(&id, generation, &next).await?;
            Ok(next)
        })
        .await
        .map_err(|_| Error::Storage)?
    }
    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        bundle: &Bundle,
    ) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.api_base))
            .bearer_auth(&bundle.access_token)
            .header("ChatGPT-Account-Id", &bundle.account_id)
            .header("originator", "codex_cli_rs")
    }
    async fn catalog(&self, bundle: &Bundle) -> Result<Vec<Model>, Error> {
        let response = self
            .request(reqwest::Method::GET, "/models?client_version=1.0.0", bundle)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(network)?;
        check_status(&response)?;
        let bytes = bounded_bytes(response).await.map_err(|error| match error {
            Error::Protocol => Error::CatalogTooLarge,
            other => other,
        })?;
        parse_models(serde_json::from_slice(&bytes).map_err(|_| Error::CatalogJson)?)
    }
    pub async fn models(self: &Arc<Self>) -> Result<Vec<Model>, Error> {
        let id = self.active_id().await?;
        let models = self.catalog(&self.credentials_for(&id).await?).await?;
        let json = serde_json::to_string(&models).map_err(|_| Error::Storage)?;
        self.store
            .save_models(&id, &json, now())
            .await
            .map_err(|_| Error::Storage)?;
        Ok(models)
    }
    pub async fn refresh_usage(self: &Arc<Self>, id: &str) -> Result<(), Error> {
        let previous = self
            .store
            .account(id)
            .await
            .map_err(|_| Error::Storage)?
            .and_then(|a| a.usage_json)
            .and_then(|s| serde_json::from_str::<super::quota::Quota>(&s).ok());
        self.store
            .mark_usage_attempt(id, now())
            .await
            .map_err(|_| Error::Storage)?;
        let result = async {
            let bundle = self.credentials_for(id).await?;
            let base = self
                .api_base
                .strip_suffix("/codex")
                .unwrap_or(&self.api_base);
            let get = |path: &str| {
                self.http
                    .get(format!("{base}/wham/{path}"))
                    .bearer_auth(&bundle.access_token)
                    .header("ChatGPT-Account-Id", &bundle.account_id)
                    .header("originator", "codex_cli_rs")
                    .header("accept", "application/json")
                    .timeout(Duration::from_secs(30))
            };
            let usage = checked_json(get("usage").send().await.map_err(network)?).await?;
            let mut quota =
                super::quota::parse_usage(&usage, now()).map_err(|_| Error::Protocol)?;
            let credits = async {
                checked_json(
                    get("rate-limit-reset-credits")
                        .send()
                        .await
                        .map_err(network)?,
                )
                .await
            }
            .await;
            match credits {
                Ok(value) => {
                    if let Some(count) = super::quota::reset_count(&value) {
                        quota.reset_credits = Some(count);
                        quota.reset_checked_at = Some(now());
                    } else {
                        quota.reset_error = Some("重置次数响应未提供已知字段".into());
                    }
                }
                Err(error) => quota.reset_error = Some(format!("重置次数查询失败：{error}")),
            }
            if quota.reset_credits.is_none()
                && let Some(previous) = previous
            {
                quota.reset_credits = previous.reset_credits;
                quota.reset_checked_at = previous.reset_checked_at;
            }
            serde_json::to_string(&quota).map_err(|_| Error::Storage)
        }
        .await;
        match result {
            Ok(json) => self
                .store
                .save_usage(id, Some(&json), None, now())
                .await
                .map_err(|_| Error::Storage),
            Err(error) => {
                self.store
                    .save_usage(id, None, Some(&error.to_string()), now())
                    .await
                    .map_err(|_| Error::Storage)?;
                Err(error)
            }
        }
    }
    pub async fn open(self: &Arc<Self>, body: &Value) -> Result<reqwest::Response, Error> {
        self.open_for(&self.active_id().await?, body).await
    }
    pub async fn open_for(
        self: &Arc<Self>,
        account: &str,
        body: &Value,
    ) -> Result<reqwest::Response, Error> {
        let bundle = self.credentials_for(account).await?;
        let models = self.catalog(&bundle).await?;
        let model = models
            .iter()
            .find(|m| Some(m.slug.as_str()) == body["model"].as_str())
            .ok_or(Error::Model)?;
        if let Some(effort) = body["reasoning"]["effort"].as_str()
            && !model.reasoning_efforts.iter().any(|s| s == effort)
        {
            return Err(Error::Protocol);
        }
        let mut request = self
            .request(reqwest::Method::POST, "/responses", &bundle)
            .header("accept", "text/event-stream");
        if let Some(cache) = body["prompt_cache_key"].as_str() {
            request = request
                .header("session-id", cache)
                .header("thread-id", cache)
                .header("x-client-request-id", cache);
        }
        let response = request.json(body).send().await.map_err(network)?;
        check_status(&response)?;
        // Some successful Codex responses omit Content-Type. The bounded SSE
        // decoder and terminal validation remain authoritative in that case.
        if response.headers().get("content-type").is_some_and(|s| {
            !s.to_str().is_ok_and(|media| {
                media
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("text/event-stream")
            })
        }) {
            let media = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim();
            return Err(Error::StreamType(match media {
                "" => "未提供 Content-Type",
                "application/json" => "JSON",
                "text/html" => "HTML",
                "text/plain" => "纯文本",
                _ => "其他非 SSE 类型",
            }));
        }
        Ok(response)
    }
}
fn check_status(r: &reqwest::Response) -> Result<(), Error> {
    match r.status().as_u16() {
        200..300 => Ok(()),
        401 => Err(Error::AuthRequired),
        n => Err(Error::Http(n)),
    }
}
async fn checked_json(response: reqwest::Response) -> Result<Value, Error> {
    check_status(&response)?;
    bounded_json(response).await
}
async fn auth_json(response: reqwest::Response, stage: &'static str) -> Result<Value, Error> {
    if !response.status().is_success() {
        return Err(Error::AuthHttp {
            stage,
            status: response.status().as_u16(),
        });
    }
    bounded_json(response).await
}
async fn device_poll(response: reqwest::Response) -> Result<Poll, Error> {
    let status = response.status().as_u16();
    if !(200..300).contains(&status) && !matches!(status, 400 | 403 | 404) {
        return Err(Error::AuthHttp {
            stage: "等待设备授权",
            status,
        });
    }
    let bytes = bounded_bytes(response).await?;
    let value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        // Match the reference's pending convention even for an empty or
        // non-JSON error body, without weakening successful token validation.
        Err(_) if matches!(status, 403 | 404) => Value::Null,
        Err(_) => return Err(Error::Protocol),
    };
    parse_poll(status, value).map_err(|error| match error {
        Error::Http(status) => Error::AuthHttp {
            stage: "等待设备授权",
            status,
        },
        other => other,
    })
}
async fn bounded_json(response: reqwest::Response) -> Result<Value, Error> {
    serde_json::from_slice(&bounded_bytes(response).await?).map_err(|_| Error::Protocol)
}
async fn bounded_bytes(response: reqwest::Response) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut bytes = Zeroizing::new(Vec::new());
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(network)?;
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(Error::Protocol);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn network(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Timeout
    } else {
        Error::Network
    }
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
