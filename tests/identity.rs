use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, jwk::JwkSet};
use rsa::{pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
use serde_json::json;
use tiny_proxy::auth::identity;

#[test]
fn signed_identity_checks_issuer_audience_expiry_nonce_and_algorithm() {
    let key = rsa::RsaPrivateKey::new(&mut rand08::thread_rng(), 2048).unwrap();
    let public = rsa::RsaPublicKey::from(&key);
    let jwks: JwkSet = serde_json::from_value(json!({"keys":[{"kty":"RSA","kid":"synthetic-test-key","use":"sig","alg":"RS256","n":URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())}]})).unwrap();
    let signing = EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes());
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("synthetic-test-key".into());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = json!({"iss":"https://auth.openai.com","aud":"oaiapp_test","sub":"synthetic-subject","nonce":"test-nonce","exp":now+3600});
    let token = encode(&header, &claims, &signing).unwrap();
    assert_eq!(
        identity::validate(&token, &jwks, "oaiapp_test", "test-nonce").unwrap(),
        "synthetic-subject"
    );
    assert!(identity::validate(&token, &jwks, "oaiapp_other", "test-nonce").is_err());
    assert!(identity::validate(&token, &jwks, "oaiapp_test", "different-nonce").is_err());
    for (name, value) in [
        ("iss", json!("https://invalid.example")),
        ("exp", json!(1)),
        ("sub", json!("")),
    ] {
        let mut invalid = claims.clone();
        invalid[name] = value;
        let token = encode(&header, &invalid, &signing).unwrap();
        assert!(identity::validate(&token, &jwks, "oaiapp_test", "test-nonce").is_err());
    }
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("synthetic-test-key".into());
    let forged = encode(
        &header,
        &claims,
        &EncodingKey::from_secret(b"not-a-production-secret"),
    )
    .unwrap();
    assert!(identity::validate(&forged, &jwks, "oaiapp_test", "test-nonce").is_err());
}
