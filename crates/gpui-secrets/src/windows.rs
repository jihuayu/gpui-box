//! The only unsafe module. Windows owns returned allocation buffers until
//! CredFree; pointer/slice construction is confined to those documented lifetimes.
use crate::{model::error, service::capabilities, *};
use gpui_system::{Availability, ErrorCode, Evidence, SystemError};
use std::{ffi::c_void, ptr};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_BUSY, ERROR_NO_SUCH_LOGON_SESSION, ERROR_NOT_FOUND,
        ERROR_NOT_SUPPORTED, GetLastError,
    },
    Security::Credentials::*,
};
use zeroize::Zeroize;

pub struct WindowsCredentials;
impl CredentialProvider for WindowsCredentials {
    fn capabilities(&self) -> Vec<gpui_system::Capability<SecretOperation>> {
        capabilities(Availability::Available, Evidence::Compiled)
    }
    fn validate_policy(&self, policy: SecretPolicy) -> Result<(), SystemError> {
        // LOCAL_MACHINE persistence is an OS credential guarantee, not hardware
        // device binding, unlock gating, biometric presence or backup exclusion.
        if policy != SecretPolicy::default() {
            return Err(error(
                ErrorCode::Unsupported,
                "Required credential protection policy is unsupported",
            ));
        }
        Ok(())
    }
    fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()> {
        validate_target(target, 4)?;
        if value.expose().len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err(error(
                ErrorCode::InvalidInput,
                "Credential exceeds native size bound",
            ));
        }
        let mut target = wide(target);
        let mut username = wide("gpui-box");
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: value.expose().len() as u32,
            CredentialBlob: value.expose().as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: username.as_mut_ptr(),
            ..CREDENTIALW::default()
        };
        // SAFETY: all borrowed buffers live through this synchronous call. The
        // OS only reads the blob; it does not own or retain the Rust pointer.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return failed();
        }
        Ok(SecretAccess::Available(()))
    }
    fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>> {
        validate_target(target, 4)?;
        let target = wide(target);
        let mut pointer = ptr::null_mut();
        // SAFETY: target is nul-terminated, output pointer is valid for the call.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) } == 0 {
            // SAFETY: read immediately after failed call; no intervening OS call.
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(SecretAccess::Available(None));
            }
            return failed();
        }
        if pointer.is_null() {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Native credential buffer is invalid",
            ));
        }
        let owned = CredentialBuffer(pointer);
        // SAFETY: successful CredReadW returns this allocation until owned drops.
        let credential = unsafe { &*owned.0 };
        if credential.Type != CRED_TYPE_GENERIC
            || credential.CredentialBlobSize as usize > MAX_SECRET_BYTES
            || (credential.CredentialBlobSize > 0 && credential.CredentialBlob.is_null())
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Native credential payload is invalid",
            ));
        }
        let bytes = if credential.CredentialBlobSize == 0 {
            Vec::new()
        } else {
            // SAFETY: OS returned a blob of CredentialBlobSize bytes owned above.
            unsafe {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
            }
            .to_vec()
        };
        Ok(SecretAccess::Available(Some(SecretBytes::new(bytes)?)))
    }
    fn delete(&self, target: &str) -> ProviderResult<DeleteReceipt> {
        validate_target(target, 4)?;
        let target = wide(target);
        // SAFETY: target is nul-terminated and the API retains no pointer.
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
            return Ok(SecretAccess::Available(DeleteReceipt { existed: true }));
        }
        // SAFETY: immediate OS error retrieval.
        if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            return Ok(SecretAccess::Available(DeleteReceipt { existed: false }));
        }
        failed()
    }
    fn list(&self, prefix: &str) -> ProviderResult<Vec<String>> {
        // Never enumerate an unscoped vault. Prefix originates from bounded,
        // encoded app and account components, with a final boundary slash.
        if !prefix.ends_with('/') {
            return Err(error(
                ErrorCode::InvalidInput,
                "Credential enumeration requires a scoped prefix",
            ));
        }
        validate_target(prefix.trim_end_matches('/'), 2)?;
        let filter = wide(&format!("{prefix}*"));
        let mut count = 0;
        let mut pointer = ptr::null_mut();
        // SAFETY: filter/output buffers are valid, Windows owns returned memory.
        if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut pointer) } == 0 {
            // SAFETY: immediate error retrieval.
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(SecretAccess::Available(Vec::new()));
            }
            return failed();
        }
        let owned = EnumerationBuffer { pointer, count };
        if count > 4096 || (count > 0 && pointer.is_null()) {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Credential enumeration exceeds its bound",
            ));
        }
        let mut targets = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            // SAFETY: API returned count pointers inside owned allocation.
            let credential = unsafe { *owned.pointer.add(index) };
            if credential.is_null() {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Native credential entry is invalid",
                ));
            }
            // SAFETY: each entry is part of the live enumeration allocation.
            let credential = unsafe { &*credential };
            if credential.Type != CRED_TYPE_GENERIC {
                continue;
            }
            let target = read_target(credential.TargetName)?;
            if !target.starts_with(prefix) {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Native enumeration escaped its namespace",
                ));
            }
            targets.push(target);
        }
        Ok(SecretAccess::Available(targets))
    }
}
fn validate_target(target: &str, components: usize) -> Result<(), SystemError> {
    let Some(suffix) = target.strip_prefix("gpui-box/v1/") else {
        return Err(error(
            ErrorCode::InvalidInput,
            "Credential namespace is invalid",
        ));
    };
    let parts: Vec<_> = suffix.split('/').collect();
    if parts.len() != components
        || parts.iter().any(|part| {
            part.is_empty()
                || part.len() > 256
                || part.len() % 2 != 0
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        })
    {
        return Err(error(
            ErrorCode::InvalidInput,
            "Credential namespace is invalid",
        ));
    }
    Ok(())
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn read_target(pointer: *const u16) -> Result<String, SystemError> {
    if pointer.is_null() {
        return Err(error(
            ErrorCode::IntegrityFailure,
            "Native credential target is invalid",
        ));
    }
    // SAFETY: Windows returns nul-terminated TargetName. Bound scanning to the
    // documented maximum target name size before forming a borrowed slice.
    let mut length = 0;
    while length < CRED_MAX_GENERIC_TARGET_NAME_LENGTH as usize {
        if unsafe { *pointer.add(length) } == 0 {
            return String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
                .map_err(|_| {
                    error(
                        ErrorCode::IntegrityFailure,
                        "Native credential target is invalid",
                    )
                });
        }
        length += 1;
    }
    Err(error(
        ErrorCode::IntegrityFailure,
        "Native credential target exceeds its bound",
    ))
}
fn failed<T>() -> ProviderResult<T> {
    // SAFETY: callers invoke immediately after a failed native credential call.
    match unsafe { GetLastError() } {
        ERROR_NO_SUCH_LOGON_SESSION => Ok(SecretAccess::Locked),
        ERROR_ACCESS_DENIED => Err(error(
            ErrorCode::PermissionDenied,
            "Secure credential access was denied",
        )),
        ERROR_BUSY => Err(error(ErrorCode::Busy, "Secure credential provider is busy")),
        ERROR_NOT_SUPPORTED => Err(error(
            ErrorCode::Unavailable,
            "Secure credential provider is unavailable",
        )),
        _ => Err(error(
            ErrorCode::PlatformFailure,
            "Secure credential operation failed",
        )),
    }
}
struct CredentialBuffer(*mut CREDENTIALW);
impl Drop for CredentialBuffer {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns a successful CredReadW allocation.
        unsafe {
            clear_blob(self.0);
            CredFree(self.0.cast::<c_void>());
        }
    }
}
struct EnumerationBuffer {
    pointer: *mut *mut CREDENTIALW,
    count: u32,
}
impl Drop for EnumerationBuffer {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: count and pointer are from successful CredEnumerateW. Clear
            // every returned secret blob before releasing the native allocation.
            unsafe {
                for index in 0..self.count as usize {
                    clear_blob(*self.pointer.add(index));
                }
                CredFree(self.pointer.cast::<c_void>());
            }
        }
    }
}
unsafe fn clear_blob(pointer: *mut CREDENTIALW) {
    if !pointer.is_null() {
        // SAFETY: caller guarantees a still-live native credential allocation.
        let credential = unsafe { &mut *pointer };
        if !credential.CredentialBlob.is_null() && credential.CredentialBlobSize > 0 {
            // SAFETY: native credential owns this mutable blob until CredFree.
            unsafe {
                std::slice::from_raw_parts_mut(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
            }
            .zeroize();
        }
    }
}
