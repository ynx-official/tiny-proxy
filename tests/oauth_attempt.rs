use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tiny_proxy::auth::attempt::{Attempt, Error};

#[test]
fn own_registration_uses_pkce_and_exact_loopback_then_consumes_callback_once() {
    let now = Instant::now();
    let mut attempt = Attempt::new(45678, "urn:uuid:synthetic-host", None, now);
    let url = attempt.authorization_url();
    let params = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
    assert_eq!(params["client_id"], "dynamic_agent_client");
    assert_eq!(params["agent_name_hint"], "Tiny Proxy");
    assert_eq!(
        params["redirect_uri"],
        "http://127.0.0.1:45678/auth/callback"
    );
    assert_eq!(params["ext_agent_host_id"], "urn:uuid:synthetic-host");
    assert!(!params.contains_key("client_secret"));
    let query = format!(
        "code=synthetic-code&client_id=oaiapp_test&state={}",
        params["state"]
    );
    let grant = attempt.callback(&query, now).unwrap();
    assert_eq!(grant.client_id, "oaiapp_test");
    assert_eq!(grant.redirect_uri, params["redirect_uri"]);
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(grant.verifier.as_bytes())),
        params["code_challenge"]
    );
    assert!(matches!(attempt.callback(&query, now), Err(Error::Expired)));
}

#[test]
fn bad_state_does_not_consume_the_attempt_but_valid_denial_does() {
    let now = Instant::now();
    let mut attempt = Attempt::new(12345, "host", None, now);
    let state = attempt
        .authorization_url()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    assert!(matches!(
        attempt.callback("error=access_denied&state=wrong", now),
        Err(Error::InvalidCallback)
    ));
    assert!(matches!(
        attempt.callback(&format!("error=access_denied&state={state}"), now),
        Err(Error::AccessDenied)
    ));
    assert!(matches!(
        attempt.callback(&format!("code=x&client_id=oaiapp_test&state={state}"), now),
        Err(Error::Expired)
    ));
}

#[test]
fn returning_registration_cannot_be_replaced_by_callback() {
    let now = Instant::now();
    let mut attempt = Attempt::new(12345, "host", Some("oaiapp_saved"), now);
    let url = attempt.authorization_url();
    let params = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
    assert!(!params.contains_key("agent_name_hint"));
    assert!(matches!(
        attempt.callback(
            &format!("code=x&client_id=oaiapp_other&state={}", params["state"]),
            now
        ),
        Err(Error::RegistrationMismatch)
    ));
    let mut attempt = Attempt::new(12345, "host", Some("oaiapp_saved"), now);
    let state = attempt
        .authorization_url()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(
        attempt
            .callback(&format!("code=x&state={state}"), now)
            .unwrap()
            .client_id,
        "oaiapp_saved"
    );
}

#[test]
fn expiry_duplicate_query_and_cancel_are_rejected() {
    let now = Instant::now();
    let mut attempt = Attempt::new(12345, "host", None, now);
    let state = attempt
        .authorization_url()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    assert!(matches!(
        attempt.callback(
            &format!("code=x&client_id=oaiapp_test&state={state}&state={state}"),
            now
        ),
        Err(Error::InvalidCallback)
    ));
    assert!(matches!(
        attempt.callback(
            &format!("code=x&client_id=oaiapp_test&state={state}"),
            now + Duration::from_secs(601)
        ),
        Err(Error::Expired)
    ));
    let mut attempt = Attempt::new(12345, "host", None, now);
    attempt.cancel();
    assert!(matches!(attempt.callback("", now), Err(Error::Expired)));
}
