use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
#[error("Windows user credential protection failed")]
pub struct ProtectionError;

pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>, ProtectionError> {
    transform(plaintext, true)
}
pub fn unprotect(ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectionError> {
    transform(ciphertext, false).map(Zeroizing::new)
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

#[cfg(not(windows))]
fn transform(_input: &[u8], _encrypt: bool) -> Result<Vec<u8>, ProtectionError> {
    // No plaintext fallback on an unimplemented platform.
    Err(ProtectionError)
}
