//! All native pointer use is restricted to clipboard-locked, bounded buffers.
use crate::*;
use std::{num::NonZeroIsize, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, GetLastError, GlobalFree, HWND, SetLastError},
    System::{
        DataExchange::{
            CloseClipboard, CountClipboardFormats, EmptyClipboard, GetClipboardData,
            GetClipboardSequenceNumber, GetOpenClipboardWindow, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsWindow},
};
const CF_UNICODETEXT: u32 = 13;

/// Factory only: applications retain `Clipboard`, not an unchecked native handle.
pub struct WindowsClipboard;
impl WindowsClipboard {
    /// `window` is a borrowed HWND for the host's current attachment. The host
    /// must invalidate `ui` before destroying/recreating that native attachment.
    pub fn attach(window: NonZeroIsize, ui: UiContext) -> Result<Clipboard, SystemError> {
        if !ui.is_current() {
            return Err(error(
                ErrorCode::ContextExpired,
                "Clipboard UI context is expired",
            ));
        }
        let provider = WindowsProvider {
            window: window.get() as HWND,
        };
        provider.validate_window(false)?;
        Clipboard::new(Rc::new(provider), ui)
    }
}
struct WindowsProvider {
    window: HWND,
}
impl WindowsProvider {
    fn validate_window(&self, foreground: bool) -> Result<(), SystemError> {
        let mut process = 0;
        // SAFETY: Win32 validates opaque HWND values; no Rust pointer is dereferenced.
        let thread = unsafe { GetWindowThreadProcessId(self.window, &mut process) };
        if unsafe { IsWindow(self.window) } == 0
            || thread != unsafe { GetCurrentThreadId() }
            || process != unsafe { GetCurrentProcessId() }
        {
            return Err(error(
                ErrorCode::ContextExpired,
                "Clipboard window attachment is invalid",
            ));
        }
        if foreground && unsafe { GetForegroundWindow() } != self.window {
            return Err(error(
                ErrorCode::PermissionDenied,
                "Clipboard access requires the foreground window",
            ));
        }
        Ok(())
    }
    fn open(&self) -> Result<ClipboardGuard, SystemError> {
        self.validate_window(true)?;
        // SAFETY: window validity/ownership was checked on its UI thread.
        if unsafe { OpenClipboard(self.window) } == 0 {
            // Another process commonly produces ERROR_ACCESS_DENIED while holding
            // the clipboard. Preserve that actionable contention distinction.
            let code = unsafe { GetLastError() };
            if code == ERROR_ACCESS_DENIED && !unsafe { GetOpenClipboardWindow() }.is_null() {
                let mut error = error(ErrorCode::Busy, "Clipboard is held by another operation");
                error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
                return Err(error);
            }
            return Err(if code == ERROR_ACCESS_DENIED {
                error(ErrorCode::PermissionDenied, "Clipboard access was denied")
            } else {
                error(ErrorCode::PlatformFailure, "Clipboard could not be opened")
            });
        }
        Ok(ClipboardGuard)
    }
}
impl ClipboardProvider for WindowsProvider {
    fn capabilities(&self) -> Vec<Capability<ClipboardOperation>> {
        // Foreground status can change immediately; only an execution check grants access.
        capabilities(
            if self.validate_window(true).is_ok() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            Support::Conditional,
            Evidence::Compiled,
        )
    }
    fn read_text(&self, context: &OperationContext) -> ServiceResult<ClipboardText> {
        let _guard = self.open()?;
        context.check_at(Instant::now())?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        self.validate_window(true)?;
        // SAFETY: this thread holds the clipboard open for the entire read.
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
            unsafe { SetLastError(0) };
            let count = unsafe { CountClipboardFormats() };
            if count == 0 && unsafe { GetLastError() } != 0 {
                return Err(native_error("Clipboard formats could not be read"));
            }
            return Ok(Completion::Completed(if count == 0 {
                ClipboardText::Empty
            } else {
                ClipboardText::NoText
            }));
        }
        let allocation = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if allocation.is_null() {
            return Err(native_error("Clipboard text could not be read"));
        }
        let bytes = unsafe { GlobalSize(allocation) };
        if bytes < 2 || bytes % 2 != 0 {
            return Err(error(
                ErrorCode::InvalidInput,
                "Native clipboard text exceeds bounds or is malformed",
            ));
        }
        let pointer = unsafe { GlobalLock(allocation) } as *const u16;
        if pointer.is_null() {
            return Err(native_error("Clipboard text could not be locked"));
        }
        let _locked = LockedAllocation(allocation);
        // SAFETY: GlobalSize bounds this live, locked allocation; aligned Windows HGLOBAL storage.
        // GlobalSize may include allocator padding. Scan at most the accepted
        // text bound plus its terminator, rather than rejecting valid text only
        // because the native allocation was rounded upward.
        let units =
            unsafe { std::slice::from_raw_parts(pointer, (bytes / 2).min(MAX_TEXT_BYTES + 1)) };
        let text = decode_text(units)?;
        Ok(Completion::Completed(ClipboardText::Text(text)))
    }
    fn write_text(
        &self,
        text: &str,
        context: &OperationContext,
    ) -> ServiceResult<ClipboardReceipt> {
        validate_text(text)?;
        // Allocate and populate before opening/emptying the existing clipboard.
        let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let allocation = unsafe { GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) };
        if allocation.is_null() {
            return Err(native_error("Clipboard text allocation failed"));
        }
        let mut owned = OwnedAllocation(allocation);
        let pointer = unsafe { GlobalLock(allocation) } as *mut u16;
        if pointer.is_null() {
            return Err(native_error(
                "Clipboard text allocation could not be locked",
            ));
        }
        // SAFETY: allocation is uniquely owned, locked, and exactly units.len()*2 bytes.
        unsafe {
            ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len());
            GlobalUnlock(allocation);
        }
        let _guard = self.open()?;
        context.check_at(Instant::now())?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        self.validate_window(true)?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(native_error("Clipboard replacement could not begin"));
        }
        if unsafe { SetClipboardData(CF_UNICODETEXT, allocation) }.is_null() {
            // EmptyClipboard already had a visible effect. Do not claim cancellation
            // or preservation of the previous formats if this native handoff fails.
            return Err(error(
                ErrorCode::PlatformFailure,
                "Clipboard publication failed after previous contents were cleared",
            ));
        }
        // Windows now owns the allocation, including after this process exits.
        owned.0 = ptr::null_mut();
        let sequence = unsafe { GetClipboardSequenceNumber() };
        Ok(Completion::Completed(ClipboardReceipt {
            sequence: (sequence != 0).then_some(sequence),
        }))
    }
}
fn decode_text(units: &[u16]) -> Result<String, SystemError> {
    let end = units.iter().position(|unit| *unit == 0).ok_or_else(|| {
        error(
            ErrorCode::InvalidInput,
            "Native clipboard text is not terminated",
        )
    })?;
    let text = String::from_utf16(&units[..end]).map_err(|_| {
        error(
            ErrorCode::InvalidInput,
            "Native clipboard text is not valid Unicode",
        )
    })?;
    validate_text(&text)?;
    Ok(text)
}
struct ClipboardGuard;
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
struct LockedAllocation(*mut std::ffi::c_void);
impl Drop for LockedAllocation {
    fn drop(&mut self) {
        unsafe {
            GlobalUnlock(self.0);
        }
    }
}
struct OwnedAllocation(*mut std::ffi::c_void);
impl Drop for OwnedAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
}
fn native_error(message: &'static str) -> SystemError {
    // SAFETY: called immediately after a failed native operation.
    let code = if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
        ErrorCode::PermissionDenied
    } else {
        ErrorCode::PlatformFailure
    };
    error(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_system::{SurfaceId, UiOwner};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE,
    };

    #[test]
    fn bounded_native_decoder_rejects_malformed_utf16_and_missing_terminator() {
        assert_eq!(
            decode_text(&[0xd800, 0]).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            decode_text(&[b'a' as u16]).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let units: Vec<u16> = "中文 🦀".encode_utf16().chain(Some(0)).collect();
        assert_eq!(decode_text(&units).unwrap(), "中文 🦀");
        assert_eq!(decode_text(&[0]).unwrap(), "");
        let mut padded = vec![b'a' as u16; MAX_TEXT_BYTES];
        padded.push(0);
        padded.extend([0; 16]);
        assert_eq!(
            decode_text(&padded[..MAX_TEXT_BYTES + 1]).unwrap().len(),
            MAX_TEXT_BYTES
        );
    }

    #[test]
    fn real_message_window_is_owned_but_cannot_access_foreground_clipboard() {
        let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
        // SAFETY: the built-in STATIC class and nul-terminated buffers are valid;
        // a message-only window has no visual UI and cannot become foreground.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!window.is_null());
        struct WindowGuard(HWND);
        impl Drop for WindowGuard {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        let _window = WindowGuard(window);
        let owner = UiOwner::new(SurfaceId(43));
        let clipboard = WindowsClipboard::attach(
            NonZeroIsize::new(window as isize).unwrap(),
            owner.context(None).unwrap(),
        )
        .unwrap();
        assert_eq!(
            clipboard.capabilities()[0].availability,
            Availability::Unavailable
        );
        assert_eq!(
            clipboard
                .read_text(clipboard.user_initiated().unwrap())
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            clipboard
                .write_text("must not publish", clipboard.user_initiated().unwrap())
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
}
