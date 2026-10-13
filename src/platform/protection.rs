use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
#[error("platform credential protection failed")]
pub struct ProtectionError;

pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>, ProtectionError> {
    transform(plaintext, true)
}
pub fn unprotect(ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectionError> {
    transform(ciphertext, false).map(Zeroizing::new)
}

/// Native keychain access can wait for a system permission prompt. Keep it
/// off the runtime owner thread so closing the desktop remains responsive.
pub async fn protect_async(plaintext: Zeroizing<Vec<u8>>) -> Result<Vec<u8>, ProtectionError> {
    tokio::task::spawn_blocking(move || protect(&plaintext))
        .await
        .map_err(|_| ProtectionError)?
}
pub async fn unprotect_async(ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectionError> {
    let ciphertext = ciphertext.to_vec();
    tokio::task::spawn_blocking(move || unprotect(&ciphertext))
        .await
        .map_err(|_| ProtectionError)?
}

#[cfg(windows)]
fn transform(input: &[u8], encrypt: bool) -> Result<Vec<u8>, ProtectionError> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    use zeroize::Zeroize;
    if input.is_empty() {
        return Err(ProtectionError);
    }
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len().try_into().map_err(|_| ProtectionError)?,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // DPAPI borrows the input and allocates output with LocalAlloc. No machine scope
    // flag: ciphertext is bound to the current Windows user's credentials.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &source,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &source,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(ProtectionError);
    }
    // A successful DPAPI call owns cbData writable bytes until LocalFree.
    let bytes = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let result = bytes.to_vec();
    bytes.zeroize();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

#[cfg(target_os = "macos")]
fn transform(input: &[u8], encrypt: bool) -> Result<Vec<u8>, ProtectionError> {
    macos::transform(input, encrypt)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::ProtectionError;
    use aes_gcm::{
        Aes256Gcm, KeyInit, Nonce,
        aead::{Aead, Payload},
    };
    use security_framework::os::macos::keychain::SecKeychain;
    use zeroize::Zeroizing;

    const SERVICE: &str = "dev.tiny-proxy.credentials";
    const ACCOUNT: &str = "local-aes-256-gcm-v1";
    const PREFIX: &[u8] = b"TPMC\x01";
    const NOT_FOUND: i32 = -25300;
    const DUPLICATE: i32 = -25299;

    fn key(create: bool) -> Result<Zeroizing<Vec<u8>>, ProtectionError> {
        let keychain = SecKeychain::default().map_err(|_| ProtectionError)?;
        let read = || {
            keychain
                .find_generic_password(SERVICE, ACCOUNT)
                .map(|(key, _)| Zeroizing::new(key.as_ref().to_vec()))
        };
        match read() {
            Ok(key) if key.len() == 32 => Ok(key),
            Err(error) if create && error.code() == NOT_FOUND => {
                let generated = Zeroizing::new(rand::random::<[u8; 32]>());
                // Add-only: simultaneous processes must never overwrite a key
                // that already protects another database's credentials.
                match keychain.add_generic_password(SERVICE, ACCOUNT, &generated[..]) {
                    Ok(()) => {}
                    Err(error) if error.code() == DUPLICATE => {}
                    Err(_) => return Err(ProtectionError),
                }
                let stored = read().map_err(|_| ProtectionError)?;
                if stored.len() != 32 {
                    return Err(ProtectionError);
                }
                Ok(stored)
            }
            _ => Err(ProtectionError),
        }
    }
    pub(super) fn transform(input: &[u8], encrypt: bool) -> Result<Vec<u8>, ProtectionError> {
        if input.is_empty() {
            return Err(ProtectionError);
        }
        if !encrypt && (!input.starts_with(PREFIX) || input.len() < PREFIX.len() + 12 + 16) {
            return Err(ProtectionError);
        }
        crypt(input, encrypt, &key(encrypt)?)
    }
    fn crypt(input: &[u8], encrypt: bool, key: &[u8]) -> Result<Vec<u8>, ProtectionError> {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ProtectionError)?;
        if encrypt {
            if input.is_empty() {
                return Err(ProtectionError);
            }
            let nonce = rand::random::<[u8; 12]>();
            let ciphertext = cipher
                .encrypt(
                    Nonce::from_slice(&nonce),
                    Payload {
                        msg: input,
                        aad: PREFIX,
                    },
                )
                .map_err(|_| ProtectionError)?;
            let mut envelope = Vec::with_capacity(PREFIX.len() + 12 + ciphertext.len());
            envelope.extend_from_slice(PREFIX);
            envelope.extend_from_slice(&nonce);
            envelope.extend_from_slice(&ciphertext);
            Ok(envelope)
        } else {
            if !input.starts_with(PREFIX) || input.len() < PREFIX.len() + 12 + 16 {
                return Err(ProtectionError);
            }
            let nonce = &input[PREFIX.len()..PREFIX.len() + 12];
            cipher
                .decrypt(
                    Nonce::from_slice(nonce),
                    Payload {
                        msg: &input[PREFIX.len() + 12..],
                        aad: PREFIX,
                    },
                )
                .map_err(|_| ProtectionError)
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn ciphertext_rejects_wrong_key_tampering_and_unknown_versions() {
            let key = [7; 32];
            let plaintext = b"synthetic-token";
            let encrypted = crypt(plaintext, true, &key).unwrap();
            assert_eq!(crypt(&encrypted, false, &key).unwrap(), plaintext);
            assert_ne!(encrypted, crypt(plaintext, true, &key).unwrap());
            assert!(crypt(&encrypted, false, &[8; 32]).is_err());
            for position in [0, PREFIX.len(), encrypted.len() - 1] {
                let mut changed = encrypted.clone();
                changed[position] ^= 1;
                assert!(crypt(&changed, false, &key).is_err());
            }
            assert!(crypt(&[], false, &key).is_err());
            assert!(crypt(&encrypted[..12], false, &key).is_err());
            assert!(
                !encrypted
                    .windows(plaintext.len())
                    .any(|part| part == plaintext)
            );
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn transform(_input: &[u8], _encrypt: bool) -> Result<Vec<u8>, ProtectionError> {
    // No plaintext fallback on an unimplemented platform.
    Err(ProtectionError)
}
