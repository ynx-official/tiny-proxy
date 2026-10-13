use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use subtle::ConstantTimeEq;

#[derive(Debug, thiserror::Error)]
#[error("ID token validation failed")]
pub struct Error;

#[derive(serde::Deserialize)]
struct Claims {
    sub: String,
    nonce: String,
}

pub fn validate(jwt: &str, keys: &JwkSet, client_id: &str, nonce: &str) -> Result<String, Error> {
    let header = decode_header(jwt).map_err(|_| Error)?;
    if header.alg != Algorithm::RS256 {
        return Err(Error);
    }
    let kid = header.kid.ok_or(Error)?;
    let key = keys.find(&kid).ok_or(Error)?;
    let key = DecodingKey::from_jwk(key).map_err(|_| Error)?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[super::attempt::ISSUER]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.validate_nbf = true;
    validation.leeway = 30;
    let claims = decode::<Claims>(jwt, &key, &validation)
        .map_err(|_| Error)?
        .claims;
    if claims.sub.is_empty() || !bool::from(claims.nonce.as_bytes().ct_eq(nonce.as_bytes())) {
        return Err(Error);
    }
    Ok(claims.sub)
}
