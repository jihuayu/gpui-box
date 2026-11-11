//! Windows 10+ HWND-based text/web-link share-sheet adapter.
//! Attach on the actual window UI thread after the host initializes a
//! single-threaded COM apartment and starts its normal Windows message pump.
use crate::*;
use ::windows::{
    ApplicationModel::DataTransfer::{DataPackage, DataRequestedEventArgs, DataTransferManager},
    Foundation::{TypedEventHandler, Uri},
    Win32::{
        Foundation::HWND,
        System::{
            Com::{APTTYPE, APTTYPE_MAINSTA, APTTYPE_STA, APTTYPEQUALIFIER, CoGetApartmentType},
            Threading::{GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::{
            Shell::IDataTransferManagerInterop,
            WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
            },
        },
    },
    core::{HSTRING, factory},
};
use gpui_system::{
    Availability, Capability, Configuration, Constraint, Evidence, PermissionState, Support,
    UiContext,
};
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const DATA_REQUEST_LIFETIME: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDataState {
    /// Invocation accepted; the OS has not requested the bounded payload yet.
    AwaitingData,
    PreparingData,
    /// Text/link representations were copied into the OS-owned DataPackage.
    /// This is neither target acceptance nor evidence of eventual delivery.
    SuppliedToOs,
    Rejected,
    Expired,
}

/// A UI-local adapter bound to one concrete attachment generation and HWND.
/// Keep it alive while a share is awaiting data. No files/resources are exposed
/// by this adapter, so dropping it revokes only its pending text/link callback.
pub struct WindowsSharing {
    hwnd: HWND,
    ui: UiContext,
    session: RefCell<Option<NativeSession>>,
    _local: PhantomData<Rc<()>>,
}
impl WindowsSharing {
    /// `hwnd` is the raw Win32 window value from the host's borrowed handle.
    /// No ownership is transferred. A destroyed/recreated attachment needs a
    /// fresh adapter; keeping a numeric handle never keeps a window alive.
    pub fn attach(hwnd: isize, ui: UiContext) -> Result<Sharing, SystemError> {
        let binding = ui.clone();
        let value = Self {
            hwnd: HWND(hwnd as *mut _),
            ui,
            session: RefCell::new(None),
            _local: PhantomData,
        };
        value.check_owner(false)?;
        Ok(Sharing::new(Rc::new(value), binding))
    }
    fn check_owner(&self, require_foreground: bool) -> Result<(), SystemError> {
        if !self.ui.is_current() {
            return Err(error(
                ErrorCode::ContextExpired,
                "Share window attachment is expired",
            ));
        }
        let mut process = 0;
        // SAFETY: these Win32 queries validate a borrowed HWND; they do not
        // dereference a Rust pointer or transfer window ownership.
        unsafe {
            if !IsWindow(Some(self.hwnd)).as_bool()
                || GetWindowThreadProcessId(self.hwnd, Some(&mut process)) != GetCurrentThreadId()
                || process != GetCurrentProcessId()
            {
                return Err(error(
                    ErrorCode::ContextExpired,
                    "Share window does not belong to the current UI thread",
                ));
            }
            if require_foreground
                && (GetForegroundWindow() != self.hwnd || !IsWindowVisible(self.hwnd).as_bool())
            {
                return Err(error(
                    ErrorCode::Unavailable,
                    "Sharing requires its visible foreground window",
                ));
            }
        }
        let mut apartment = APTTYPE::default();
        let mut qualifier = APTTYPEQUALIFIER::default();
        // SAFETY: output storage is valid; querying COM never initializes or
        // uninitializes the host's apartment and requires no balancing call.
        unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }.map_err(|_| {
            error(
                ErrorCode::InvalidConfiguration,
                "The host must initialize its UI COM apartment",
            )
        })?;
        if apartment != APTTYPE_STA && apartment != APTTYPE_MAINSTA {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Windows sharing requires a single-threaded UI apartment",
            ));
        }
        Ok(())
    }
    /// Observation of payload preparation only; never a delivery receipt.
    /// Expiration is polled here and before the next invocation. The listener
    /// also rejects an expired callback even if the host never polls this state.
    pub fn data_state(&self) -> Option<NativeDataState> {
        self.session
            .borrow()
            .as_ref()
            .map(|session| session.pending.state())
    }
    fn retire_finished(&self) -> Result<(), SystemError> {
        let previous = self.session.borrow_mut().take();
        if previous.as_ref().is_some_and(|session| {
            session.pending.state() == NativeDataState::AwaitingData
                || session.pending.state() == NativeDataState::PreparingData
        }) {
            *self.session.borrow_mut() = previous;
            return Err(error(
                ErrorCode::Busy,
                "The previous share is still awaiting its native data request",
            ));
        }
        // Native unsubscription/drop occurs without the RefCell borrowed.
        drop(previous);
        Ok(())
    }
}
impl SharingProvider for WindowsSharing {
    fn capability(&self, operation: ShareOperation) -> Capability<ShareOperation> {
        if !matches!(operation, ShareOperation::Text | ShareOperation::Url) {
            return unsupported(operation);
        }
        let owner = self.check_owner(true);
        let supported = DataTransferManager::IsSupported().unwrap_or(false);
        Capability {
            operation,
            support: Support::Conditional,
            availability: if owner.is_ok() && supported {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            configuration: if owner
                .as_ref()
                .is_err_and(|e| e.code == ErrorCode::InvalidConfiguration)
            {
                Configuration::Required
            } else {
                Configuration::Ready
            },
            permission: PermissionState::NotRequired,
            evidence: Evidence::Compiled,
            constraints: vec![
                Constraint::ForegroundOwner,
                Constraint::MinimumOsVersion("Windows 10"),
                Constraint::Provider("Single text or HTTP(S) link; UI/target outcome unobservable"),
            ],
            observed_revision: 1,
        }
    }
    fn share(
        &self,
        request: &ShareRequest,
        context: &OperationContext,
    ) -> ServiceResult<ShareReceipt> {
        self.check_owner(true)?;
        let ui = context.require_ui()?;
        if ui.surface != self.ui.surface || ui.generation != self.ui.generation {
            return Err(error(
                ErrorCode::ContextExpired,
                "Sharing context belongs to another attachment",
            ));
        }
        request.validate()?;
        if request.route != ShareRoute::Native
            || request.items.len() != 1
            || matches!(request.items[0], ShareItem::Resource(_))
        {
            return Err(error(
                ErrorCode::Unsupported,
                "Windows native sharing requires one text or web-link item",
            ));
        }
        self.retire_finished()?;
        let interop =
            factory::<DataTransferManager, IDataTransferManagerInterop>().map_err(native_error)?;
        // SAFETY: current UI thread/process HWND validated above. COM apartment
        // is host-owned and verified, and result owns its interface reference.
        let manager: DataTransferManager =
            unsafe { interop.GetForWindow(self.hwnd) }.map_err(native_error)?;
        let pending = Arc::new(PendingData::new(request.clone(), context.clone()));
        let callback = pending.clone();
        let token = manager
            .DataRequested(&TypedEventHandler::<
                DataTransferManager,
                DataRequestedEventArgs,
            >::new(move |_, args| {
                let Some(args) = args.as_ref() else {
                    callback.reject();
                    return Ok(());
                };
                let native = args.Request()?;
                let Some(payload) = callback.take() else {
                    native.FailWithDisplayText(&HSTRING::from(
                        "This share request is no longer available",
                    ))?;
                    return Ok(());
                };
                match make_package(&payload) {
                    Ok(package) => match native.SetData(&package) {
                        Ok(()) => callback.supplied(),
                        Err(_) => {
                            callback.reject();
                            native.FailWithDisplayText(&HSTRING::from(
                                "Unable to prepare this share request",
                            ))?;
                        }
                    },
                    Err(_) => {
                        callback.reject();
                        native.FailWithDisplayText(&HSTRING::from(
                            "Unable to prepare this share request",
                        ))?;
                    }
                }
                Ok(())
            }))
            .map_err(native_error)?;
        let session = NativeSession {
            manager,
            token,
            pending,
        };
        context.check_at(Instant::now())?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        self.check_owner(true)?;
        // Install lifetime state before invoking native code, but release the
        // RefCell borrow because DataRequested can reenter synchronously.
        *self.session.borrow_mut() = Some(session);
        // SAFETY: validated current foreground HWND, retained listener above.
        if let Err(cause) = unsafe { interop.ShowShareUIForWindow(self.hwnd) } {
            let failed = self.session.borrow_mut().take();
            drop(failed);
            return Err(native_error(cause));
        }
        Ok(Completion::Completed(
            ShareReceipt::SubmittedOutcomeUnobservable,
        ))
    }
}
fn make_package(payload: &ShareRequest) -> ::windows::core::Result<DataPackage> {
    let package = DataPackage::new()?;
    package
        .Properties()?
        .SetTitle(&HSTRING::from(payload.title.as_deref().unwrap_or("Share")))?;
    match &payload.items[0] {
        ShareItem::Text(text) => package.SetText(&HSTRING::from(text))?,
        ShareItem::Url(url) => {
            package.SetWebLink(&Uri::CreateUri(&HSTRING::from(url.as_str()))?)?
        }
        ShareItem::Resource(_) => {
            return Err(::windows::core::Error::from_hresult(
                ::windows::core::HRESULT(0x80004001u32 as i32),
            ));
        }
    }
    Ok(package)
}
struct NativeSession {
    manager: DataTransferManager,
    token: i64,
    pending: Arc<PendingData>,
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        self.pending.reject();
        let _ = self.manager.RemoveDataRequested(self.token);
    }
}
struct PendingData {
    data: Mutex<(NativeDataState, Option<ShareRequest>)>,
    context: OperationContext,
    expires: Instant,
}
impl PendingData {
    fn new(request: ShareRequest, context: OperationContext) -> Self {
        Self {
            data: Mutex::new((NativeDataState::AwaitingData, Some(request))),
            context,
            expires: Instant::now() + DATA_REQUEST_LIFETIME,
        }
    }
    fn state(&self) -> NativeDataState {
        let discarded = {
            let mut data = self
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if data.0 == NativeDataState::AwaitingData && Instant::now() >= self.expires {
                data.0 = NativeDataState::Expired;
                data.1.take()
            } else {
                None
            }
        };
        drop(discarded);
        self.data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0
    }
    fn take(&self) -> Option<ShareRequest> {
        if self.state() != NativeDataState::AwaitingData {
            return None;
        }
        // An accepted invocation is not undone by later caller cancellation.
        if self.context.ui.as_ref().is_none_or(|ui| !ui.is_current()) {
            self.reject();
            return None;
        }
        let mut data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if data.0 != NativeDataState::AwaitingData {
            return None;
        }
        data.0 = NativeDataState::PreparingData;
        data.1.take()
    }
    fn supplied(&self) {
        let mut data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if data.0 == NativeDataState::PreparingData {
            data.0 = NativeDataState::SuppliedToOs;
        }
    }
    fn reject(&self) {
        let discarded = {
            let mut data = self
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if matches!(
                data.0,
                NativeDataState::AwaitingData | NativeDataState::PreparingData
            ) {
                data.0 = NativeDataState::Rejected;
            }
            data.1.take()
        };
        drop(discarded);
    }
}
fn native_error(cause: ::windows::core::Error) -> SystemError {
    let code = match cause.code().0 as u32 {
        0x80070005 => ErrorCode::PermissionDenied,
        0x80040154 | 0x80004001 => ErrorCode::Unavailable,
        0x80010106 | 0x800401F0 => ErrorCode::InvalidConfiguration,
        0x80070057 => ErrorCode::InvalidInput,
        _ => ErrorCode::PlatformFailure,
    };
    error(code, "Windows sharing operation failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_system::{CancelReason, SurfaceId, UiOwner};
    fn pending(owner: &UiOwner) -> PendingData {
        PendingData::new(
            ShareRequest::native(ShareItem::Text("中文 😀".into())),
            OperationContext {
                ui: Some(owner.context(None).unwrap()),
                ..Default::default()
            },
        )
    }
    #[test]
    fn native_data_is_one_shot_and_post_handoff_cancel_is_not_revocation() {
        let owner = UiOwner::new(SurfaceId(1));
        let pending = pending(&owner);
        pending.context.cancellation.cancel(CancelReason::Caller);
        assert!(pending.take().is_some());
        assert_eq!(pending.state(), NativeDataState::PreparingData);
        assert!(pending.take().is_none());
        pending.supplied();
        assert_eq!(pending.state(), NativeDataState::SuppliedToOs);
        assert!(pending.take().is_none());
    }
    #[test]
    fn stale_attachment_expiry_and_drop_reject_late_data() {
        let owner = UiOwner::new(SurfaceId(1));
        let stale = pending(&owner);
        owner.advance_generation().unwrap();
        assert!(stale.take().is_none());
        assert_eq!(stale.state(), NativeDataState::Rejected);
        let mut expired = pending(&owner);
        expired.expires = Instant::now() - Duration::from_secs(1);
        assert!(expired.take().is_none());
        assert_eq!(expired.state(), NativeDataState::Expired);
        let abandoned = pending(&owner);
        abandoned.reject();
        assert!(abandoned.take().is_none());
    }
    #[test]
    fn null_or_foreign_window_cannot_attach() {
        let owner = UiOwner::new(SurfaceId(1));
        assert_eq!(
            WindowsSharing::attach(0, owner.context(None).unwrap())
                .err()
                .unwrap()
                .code,
            ErrorCode::ContextExpired
        );
    }
}
