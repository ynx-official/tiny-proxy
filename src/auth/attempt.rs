use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use url::Url;
use zeroize::Zeroizing;

pub const AUTH_ENDPOINT: &str = "https://auth.openai.com/api/accounts/authorize";
pub const TOKEN_ENDPOINT: &str = "https://auth.openai.com/api/accounts/oauth/token";
pub const RESOURCE: &str = "https://api.openai.com/v1";
pub const ISSUER: &str = "https://auth.openai.com";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid OAuth callback")]
    InvalidCallback,
    #[error("OAuth attempt expired or already used")]
    Expired,
    #[error("authorization was declined")]
    AccessDenied,
    #[error("OAuth callback belongs to another registration")]
    RegistrationMismatch,
}

pub struct CodeGrant {
    pub client_id: String,
    pub code: Zeroizing<String>,
    pub verifier: Zeroizing<String>,
    pub nonce: Zeroizing<String>,
    pub redirect_uri: String,
}

pub struct Attempt {
    host_id: String,
    client_id: Option<String>,
    redirect_uri: String,
    state: Zeroizing<String>,
    nonce: Zeroizing<String>,
    verifier: Zeroizing<String>,
    deadline: Instant,
    consumed: bool,
}
impl Attempt {
    pub fn new(port: u16, host_id: &str, client_id: Option<&str>, now: Instant) -> Self {
        let secret = || Zeroizing::new(URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>()));
        Self {
            host_id: host_id.into(),
            client_id: client_id.map(str::to_owned),
            redirect_uri: format!("http://127.0.0.1:{port}/auth/callback"),
            state: secret(),
            nonce: secret(),
            verifier: secret(),
            deadline: now + Duration::from_secs(600),
            consumed: false,
        }
    }
    pub fn authorization_url(&self) -> Url {
        let mut url = Url::parse(AUTH_ENDPOINT).expect("fixed authorization URL");
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()));
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair(
                    "client_id",
                    self.client_id.as_deref().unwrap_or("dynamic_agent_client"),
                )
                .append_pair("ext_agent_host_id", &self.host_id)
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &self.redirect_uri)
                .append_pair("resource", RESOURCE)
                .append_pair(
                    "scope",
                    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
                )
                .append_pair("state", &self.state)
                .append_pair("nonce", &self.nonce)
                .append_pair("code_challenge_method", "S256")
                .append_pair("code_challenge", &challenge);
            if self.client_id.is_none() {
                query.append_pair("agent_name_hint", "Tiny Proxy");
            }
        }
        url
    }
    pub fn callback(&mut self, query: &str, now: Instant) -> Result<CodeGrant, Error> {
        if self.consumed || now >= self.deadline {
            return Err(Error::Expired);
        }
        if query.len() > 16 * 1024 {
            return Err(Error::InvalidCallback);
        }
        let mut fields = HashMap::new();
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            if fields
                .insert(key.into_owned(), Zeroizing::new(value.into_owned()))
                .is_some()
            {
                return Err(Error::InvalidCallback);
            }
        }
        let state = fields.get("state").ok_or(Error::InvalidCallback)?;
        if !bool::from(state.as_bytes().ct_eq(self.state.as_bytes())) {
            return Err(Error::InvalidCallback);
        }
        self.consumed = true;
        if fields.contains_key("error") {
            return Err(Error::AccessDenied);
        }
        let callback_id = fields.get("client_id").map(|v| v.as_str());
        let id = match (&self.client_id, callback_id) {
            (Some(saved), None) => saved.clone(),
            (Some(saved), Some(returned)) if saved == returned => saved.clone(),
            (Some(_), Some(_)) => return Err(Error::RegistrationMismatch),
            (None, Some(issued)) if issued.starts_with("oaiapp_") && issued.len() <= 256 => {
                issued.to_owned()
            }
            _ => return Err(Error::InvalidCallback),
        };
        let code = fields
            .remove("code")
            .filter(|v| !v.is_empty())
            .ok_or(Error::InvalidCallback)?;
        Ok(CodeGrant {
            client_id: id,
            code,
            verifier: self.verifier.clone(),
            nonce: self.nonce.clone(),
            redirect_uri: self.redirect_uri.clone(),
        })
    }
    pub fn cancel(&mut self) {
        self.consumed = true;
    }
}
