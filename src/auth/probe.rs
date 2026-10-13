//! Isolated P0 sign-in and text inference experiment. Automatic refresh, remote
//! revocation and function-tool round trips belong to later validated slices.
use super::{
    attempt::{CodeGrant, ISSUER, RESOURCE, TOKEN_ENDPOINT},
    identity,
};
use crate::{
    platform::protection,
    protocol::{responses::TextProbe, sse::Decoder},
    storage::Store,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("尚未登录，请先连接 ChatGPT")]
    NoAccount,
    #[error("凭据已过期；当前验证版需要重新登录，自动续期尚未实现")]
    Expired,
    #[error("登录成功，但尚未授予模型调用权限")]
    NoPermission,
    #[error("网络请求失败，请检查连接后手动重试")]
    Network,
    #[error("上游拒绝请求（HTTP {0}）；未自动重试")]
    Http(u16),
    #[error("身份校验失败，原连接凭据未替换")]
    Identity,
    #[error("本地凭据保护或存储失败，未使用明文替代")]
    Storage,
    #[error("上游响应未通过协议或完整性校验")]
    Protocol,
    #[error("所选模型不在当前账号的目录中")]
    Model,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    id_token: String,
    token_type: String,
    scope: String,
    expires_in: i64,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct Bundle {
    access_token: String,
    refresh_token: String,
    id_token: String,
    scope: String,
    subject: String,
    client_id: String,
    expires_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub slug: String,
    pub display_name: String,
    pub visibility: String,
}

pub struct ProbeResult {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

fn client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| Error::Network)
}

async fn bounded_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, Error> {
    if !response.status().is_success() {
        return Err(Error::Http(response.status().as_u16()));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| Error::Network)?;
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(Error::Protocol);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)
}

pub async fn sign_in(store: &Store, grant: CodeGrant) -> Result<bool, Error> {
    // Retain the issued registration even if exchange fails; reauthorization must
    // use this ID rather than creating another dynamic registration each time.
    let id = store
        .retain_registration(&grant.client_id)
        .await
        .map_err(|_| Error::Storage)?;
    let http = client()?;
    let token: TokenReply = bounded_json(
        http.post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", grant.client_id.as_str()),
                ("code", grant.code.as_str()),
                ("code_verifier", grant.verifier.as_str()),
                ("redirect_uri", grant.redirect_uri.as_str()),
                ("resource", RESOURCE),
            ])
            .send()
            .await
            .map_err(|_| Error::Network)?,
    )
    .await?;
    if !token.token_type.eq_ignore_ascii_case("bearer")
        || token.access_token.is_empty()
        || token.refresh_token.is_empty()
        || token.expires_in <= 0
        || token.expires_in > 86400
    {
        return Err(Error::Protocol);
    }
    #[derive(Deserialize)]
    struct Discovery {
        issuer: String,
        jwks_uri: String,
    }
    let discovery: Discovery = bounded_json(
        http.get(format!("{ISSUER}/.well-known/openid-configuration"))
            .send()
            .await
            .map_err(|_| Error::Network)?,
    )
    .await?;
    let jwks_url = url::Url::parse(&discovery.jwks_uri).map_err(|_| Error::Identity)?;
    if discovery.issuer != ISSUER
        || jwks_url.scheme() != "https"
        || jwks_url.host_str() != Some("auth.openai.com")
        || jwks_url.port_or_known_default() != Some(443)
        || !jwks_url.username().is_empty()
        || jwks_url.password().is_some()
    {
        return Err(Error::Identity);
    }
    let keys = bounded_json(
        http.get(jwks_url)
            .send()
            .await
            .map_err(|_| Error::Network)?,
    )
    .await?;
    let subject = identity::validate(&token.id_token, &keys, &grant.client_id, &grant.nonce)
        .map_err(|_| Error::Identity)?;
    let inference = token
        .scope
        .split_whitespace()
        .any(|s| s == "chatgpt.tokens.use.direct");
    let bundle = Bundle {
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.clone(),
        id_token: token.id_token.clone(),
        scope: token.scope.clone(),
        subject: subject.clone(),
        client_id: grant.client_id.clone(),
        expires_at: now() + token.expires_in,
    };
    let encoded = Zeroizing::new(serde_json::to_vec(&bundle).map_err(|_| Error::Storage)?);
    let blob = tokio::task::spawn_blocking(move || protection::protect(&encoded))
        .await
        .map_err(|_| Error::Storage)?
        .map_err(|_| Error::Storage)?;
    store
        .complete_sign_in(&id, &subject, inference, &blob, bundle.expires_at)
        .await
        .map_err(|_| Error::Storage)?;
    Ok(inference)
}

async fn active_bundle(store: &Store) -> Result<Bundle, Error> {
    let registration = store
        .registrations()
        .await
        .map_err(|_| Error::Storage)?
        .into_iter()
        .find(|r| r.active)
        .ok_or(Error::NoAccount)?;
    let (_, bytes) = store
        .load_credentials(&registration.id)
        .await
        .map_err(|_| Error::Storage)?
        .ok_or(Error::NoAccount)?;
    let plaintext = tokio::task::spawn_blocking(move || protection::unprotect(&bytes))
        .await
        .map_err(|_| Error::Storage)?
        .map_err(|_| Error::Storage)?;
    let bundle: Bundle = serde_json::from_slice(&plaintext).map_err(|_| Error::Storage)?;
    if bundle.client_id != registration.client_id {
        return Err(Error::Identity);
    }
    if bundle.expires_at <= now() + 30 {
        return Err(Error::Expired);
    }
    if !bundle
        .scope
        .split_whitespace()
        .any(|s| s == "chatgpt.tokens.use.direct")
    {
        return Err(Error::NoPermission);
    }
    Ok(bundle)
}

async fn catalog(http: &reqwest::Client, token: &str) -> Result<Vec<Model>, Error> {
    #[derive(Deserialize)]
    struct Catalog {
        models: Vec<Model>,
    }
    let data: Catalog = bounded_json(
        http.get(format!("{RESOURCE}/models"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| Error::Network)?,
    )
    .await?;
    Ok(data
        .models
        .into_iter()
        .filter(|m| m.visibility == "list" && !m.slug.is_empty())
        .collect())
}

pub async fn models(store: &Store) -> Result<Vec<Model>, Error> {
    let bundle = active_bundle(store).await?;
    catalog(&client()?, &bundle.access_token).await
}

pub async fn test_text(store: &Store, model: &str) -> Result<ProbeResult, Error> {
    let bundle = active_bundle(store).await?;
    let http = client()?;
    if !catalog(&http, &bundle.access_token)
        .await?
        .iter()
        .any(|m| m.slug == model)
    {
        return Err(Error::Model);
    }
    let response = http.post(format!("{RESOURCE}/responses")).timeout(Duration::from_secs(180))
        .bearer_auth(&bundle.access_token).json(&json!({
            "model": model, "input": [{"role":"user","content":"Reply with exactly: Tiny Proxy connection OK"}],
            "store":false, "stream":true
        })).send().await.map_err(|_| Error::Network)?;
    if !response.status().is_success() {
        return Err(Error::Http(response.status().as_u16()));
    }
    if response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(';').next())
        .is_none_or(|s| !s.trim().eq_ignore_ascii_case("text/event-stream"))
    {
        return Err(Error::Protocol);
    }
    let mut stream = response.bytes_stream();
    let mut decoder = Decoder::new(1024 * 1024);
    let mut probe = TextProbe::new(1024 * 1024);
    let mut received = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| Error::Network)?;
        received += chunk.len();
        if received > 4 * 1024 * 1024 {
            return Err(Error::Protocol);
        }
        for event in decoder.push(&chunk).map_err(|_| Error::Protocol)? {
            probe.accept(event).map_err(|_| Error::Protocol)?;
        }
    }
    decoder.finish().map_err(|_| Error::Protocol)?;
    let (text, response) = probe.finish().map_err(|_| Error::Protocol)?;
    Ok(ProbeResult {
        text,
        input_tokens: response["usage"]["input_tokens"].as_u64(),
        output_tokens: response["usage"]["output_tokens"].as_u64(),
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
