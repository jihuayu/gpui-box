//! Explicit file-provider contract and typed picker completion.

use crate::{
    FileOperation, FileResource, GrantRestoreOutcome, OpenFileOptions, OpenFilesOptions,
    PersistedGrant, PickDirectoryOptions, PickerOutcome, SaveFileOptions,
};
use gpui_system::{
    Capability, Completion, CompletionPort, ErrorCode, RequestId, SurfaceId, SystemError,
};
use std::{
    cell::Cell,
    fmt,
    rc::{Rc, Weak},
};

/// Provider-facing description of one interactive picker invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePickerRequest {
    pub request: RequestId,
    pub operation: FileOperation,
    pub surface: SurfaceId,
}

struct CompletionState {
    completed: Cell<bool>,
}

/// Cloneable provider callback for a picker. It stays UI-local because the
/// service state and native dialog objects are not `Send`.
///
/// Exactly one of `complete`, `complete_all`, `cancel` or `fail` is accepted.
/// Stale generations, disposed owners, cancellation and duplicate callbacks
/// return false and never complete the request.
pub struct PickerCompletion<T> {
    port: CompletionPort<PickerOutcome<T>>,
    service: Weak<crate::service::ServiceInner>,
    token: u64,
    operation: FileOperation,
    surface: SurfaceId,
    state: Rc<CompletionState>,
}

impl<T> Clone for PickerCompletion<T> {
    fn clone(&self) -> Self {
        Self {
            port: self.port.clone(),
            service: self.service.clone(),
            token: self.token,
            operation: self.operation,
            surface: self.surface,
            state: Rc::clone(&self.state),
        }
    }
}

impl<T> PickerCompletion<T> {
    pub(crate) fn new(
        port: CompletionPort<PickerOutcome<T>>,
        service: Weak<crate::service::ServiceInner>,
        token: u64,
        operation: FileOperation,
        surface: SurfaceId,
    ) -> Self {
        Self {
            port,
            service,
            token,
            operation,
            surface,
            state: Rc::new(CompletionState {
                completed: Cell::new(false),
            }),
        }
    }

    pub fn id(&self) -> RequestId {
        self.port.id()
    }

    pub const fn operation(&self) -> FileOperation {
        self.operation
    }

    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Complete as an ordinary user dismissal.
    pub fn cancel(&self) -> bool {
        self.finish(None, 0)
    }

    /// Complete with a typed provider failure. Return `Err` from the provider
    /// method instead when no dialog was presented.
    pub fn fail(&self, error: SystemError) -> bool {
        if self.state.completed.replace(true) {
            return false;
        }
        let Some(service) = self.service.upgrade() else {
            return false;
        };
        if !service.finish_picker(
            self.token,
            self.operation,
            self.surface,
            crate::service::PickerFinish::Failed(error.code),
        ) {
            return false;
        }
        self.port.complete(Err(error))
    }

    fn finish(&self, value: Option<T>, selected: usize) -> bool {
        if self.state.completed.replace(true) {
            return false;
        }
        let Some(service) = self.service.upgrade() else {
            return false;
        };
        let finish = match value {
            Some(_) => crate::service::PickerFinish::Selected(selected),
            None => crate::service::PickerFinish::Cancelled,
        };
        if !service.finish_picker(self.token, self.operation, self.surface, finish) {
            return false;
        }
        let outcome = match value {
            Some(value) => PickerOutcome::Selected(value),
            None => PickerOutcome::Cancelled,
        };
        self.port.complete(Ok(Completion::Completed(outcome)))
    }
}

impl<T: PickerSelection> PickerCompletion<T> {
    /// Complete with a selected value. The service validates the picker lane
    /// before accepting the callback and the lifecycle event reports the exact
    /// number of selected resources.
    pub fn complete(&self, value: T) -> bool {
        let selected = value.selected_count();
        if selected == 0 {
            return self.cancel();
        }
        self.finish(Some(value), selected)
    }
}

/// Counts the resources in a typed picker result.
pub trait PickerSelection {
    fn selected_count(&self) -> usize;
}

impl PickerSelection for FileResource {
    fn selected_count(&self) -> usize {
        1
    }
}

impl PickerSelection for Vec<FileResource> {
    fn selected_count(&self) -> usize {
        self.len()
    }
}

impl<T> fmt::Debug for PickerCompletion<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PickerCompletion")
            .field("request", &self.port.id())
            .field("operation", &self.operation)
            .field("surface", &self.surface)
            .field("completed", &self.state.completed.get())
            .finish()
    }
}

impl PickerCompletion<Vec<FileResource>> {
    /// Complete a multi-selection. Equivalent to [`Self::complete`].
    pub fn complete_all(&self, values: Vec<FileResource>) -> bool {
        self.complete(values)
    }
}

/// The explicit provider boundary for authorized files.
///
/// Interactive methods must return `Err` only when no dialog was presented.
/// They must not block the UI thread: retain the completion and call it from
/// the platform callback, or call it synchronously only when the platform
/// resolves the dialog inline. Providers never claim system-picker grants as
/// app-wide storage permissions.
pub trait FileProvider {
    /// Describe an operation without presenting a dialog or touching a resource.
    fn capability(&self, operation: FileOperation) -> Capability<FileOperation>;

    fn open_file(
        &self,
        options: OpenFileOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<FileResource>,
    ) -> Result<(), SystemError>;

    fn open_files(
        &self,
        options: OpenFilesOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<Vec<FileResource>>,
    ) -> Result<(), SystemError>;

    fn pick_directory(
        &self,
        options: PickDirectoryOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<FileResource>,
    ) -> Result<(), SystemError>;

    fn save_file(
        &self,
        options: SaveFileOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<FileResource>,
    ) -> Result<(), SystemError>;

    /// Resolve a durable grant without presenting UI. Re-prompting is the
    /// application's responsibility for every non-restored outcome.
    fn restore_grant(&self, grant: &PersistedGrant) -> Result<GrantRestoreOutcome, SystemError>;

    /// Operations re-probed by diagnostics or a future resume reconciliation.
    fn tracked_operations(&self) -> Vec<FileOperation> {
        FileOperation::ALL.to_vec()
    }
}

/// Build a typed failure for a provider that has no native implementation.
pub fn unsupported_provider_operation(_operation: FileOperation) -> SystemError {
    SystemError::new(
        ErrorCode::Unsupported,
        "This provider does not implement the requested file operation",
    )
}
