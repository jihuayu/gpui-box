//! Optional GPUI integration for [`FileService`].
//!
//! All picker calls must run on the owning GPUI UI thread. Streaming helpers
//! move only owned, `Send` resources and contexts to GPUI's background
//! executor; no dialog or full-file copy runs on the UI thread.

use crate::{
    ExportResourceLease, FileEvent, FileProvider, FileService, OpenFileOptions, OpenFilesOptions,
    PickDirectoryOptions, PickerOutcome, SaveFileOptions, WriteOptions, WriteReceipt,
};
use gpui::{App, AsyncApp, BackgroundExecutor, Context, PathPromptOptions, SharedString, Task};
use gpui_system::{
    EventSubscription, OperationContext, Request, ServiceResult, SystemError, SystemServices,
    UiOwner, gpui_adapter,
};
use std::rc::Rc;

/// Register one explicit file service. Call before starting operations.
pub fn install_file_service(
    services: &mut SystemServices,
    provider: Rc<dyn FileProvider>,
) -> Result<(), SystemError> {
    services.register(FileService::new(provider))
}

/// Access the installed service from GPUI's typed `SystemServices` global.
pub fn file_service(cx: &App) -> Result<Rc<FileService>, SystemError> {
    cx.global::<SystemServices>().service::<FileService>()
}

/// Run a file request on GPUI's local foreground executor with the runtime
/// deadline timer installed.
pub fn run_file_request<T: 'static>(request: Request<T>, cx: &App) -> Task<ServiceResult<T>> {
    gpui_adapter::run_request(request, cx)
}

/// Foreground picker helper: the dialog remains on the UI thread and the
/// returned task supports cooperative cancellation in `context`.
pub fn open_file(
    service: &FileService,
    options: OpenFileOptions,
    context: OperationContext,
    cx: &App,
) -> Task<ServiceResult<PickerOutcome<crate::FileResource>>> {
    run_file_request(service.open_file(options, context), cx)
}

pub fn open_files(
    service: &FileService,
    options: OpenFilesOptions,
    context: OperationContext,
    cx: &App,
) -> Task<ServiceResult<PickerOutcome<Vec<crate::FileResource>>>> {
    run_file_request(service.open_files(options, context), cx)
}

pub fn pick_directory(
    service: &FileService,
    options: PickDirectoryOptions,
    context: OperationContext,
    cx: &App,
) -> Task<ServiceResult<PickerOutcome<crate::FileResource>>> {
    run_file_request(service.pick_directory(options, context), cx)
}

pub fn save_file(
    service: &FileService,
    options: SaveFileOptions,
    context: OperationContext,
    cx: &App,
) -> Task<ServiceResult<PickerOutcome<crate::FileResource>>> {
    run_file_request(service.save_file(options, context), cx)
}

impl From<&OpenFileOptions> for PathPromptOptions {
    fn from(options: &OpenFileOptions) -> Self {
        Self {
            files: true,
            directories: false,
            multiple: false,
            prompt: options.prompt.as_ref().map(SharedString::from),
        }
    }
}

impl From<OpenFileOptions> for PathPromptOptions {
    fn from(options: OpenFileOptions) -> Self {
        Self::from(&options)
    }
}

impl From<&OpenFilesOptions> for PathPromptOptions {
    fn from(options: &OpenFilesOptions) -> Self {
        Self {
            files: true,
            directories: false,
            multiple: true,
            prompt: options.prompt.as_ref().map(SharedString::from),
        }
    }
}

impl From<OpenFilesOptions> for PathPromptOptions {
    fn from(options: OpenFilesOptions) -> Self {
        Self::from(&options)
    }
}

impl From<&PickDirectoryOptions> for PathPromptOptions {
    fn from(options: &PickDirectoryOptions) -> Self {
        Self {
            files: false,
            directories: true,
            multiple: false,
            prompt: options.prompt.as_ref().map(SharedString::from),
        }
    }
}

impl From<PickDirectoryOptions> for PathPromptOptions {
    fn from(options: PickDirectoryOptions) -> Self {
        Self::from(&options)
    }
}

impl From<&SaveFileOptions> for PathPromptOptions {
    fn from(options: &SaveFileOptions) -> Self {
        Self {
            files: true,
            directories: false,
            multiple: false,
            prompt: options.prompt.as_ref().map(SharedString::from),
        }
    }
}

impl From<SaveFileOptions> for PathPromptOptions {
    fn from(options: SaveFileOptions) -> Self {
        Self::from(&options)
    }
}

/// Convert typed options to the exact workspace GPUI prompt shape. Extension
/// filters have no `PathPromptOptions` field in this GPUI revision; providers
/// remain responsible for post-selection validation.
pub fn path_prompt_options(options: impl Into<PathPromptOptions>) -> PathPromptOptions {
    options.into()
}

fn background_spawn<T: Send + 'static>(
    executor: &BackgroundExecutor,
    work: impl FnOnce() -> T + Send + 'static,
) -> Task<T> {
    executor.spawn(async move { work() })
}

/// Stream a resource's chunks on the background executor.
pub fn spawn_read_chunks(
    cx: &App,
    resource: crate::FileResource,
    context: OperationContext,
    chunk_bytes: usize,
    sink: impl FnMut(&[u8]) + Send + 'static,
) -> Task<ServiceResult<u64>> {
    let executor = cx.background_executor();
    background_spawn(executor, move || {
        resource.read_chunks(&context, chunk_bytes, sink)
    })
}

/// `AsyncApp` form for use inside an existing foreground task.
pub fn spawn_read_chunks_async(
    cx: &AsyncApp,
    resource: crate::FileResource,
    context: OperationContext,
    chunk_bytes: usize,
    sink: impl FnMut(&[u8]) + Send + 'static,
) -> Task<ServiceResult<u64>> {
    background_spawn(cx.background_executor(), move || {
        resource.read_chunks(&context, chunk_bytes, sink)
    })
}

/// Save owned bytes through the resource's atomic staging path on the
/// background executor.
pub fn spawn_save_bytes(
    cx: &App,
    resource: crate::FileResource,
    bytes: Vec<u8>,
    options: WriteOptions,
    context: OperationContext,
) -> Task<ServiceResult<WriteReceipt>> {
    let executor = cx.background_executor();
    background_spawn(executor, move || {
        let mut cursor = std::io::Cursor::new(bytes);
        resource.save_from_reader(&mut cursor, options, &context)
    })
}

/// `AsyncApp` form for use inside an existing foreground task.
pub fn spawn_save_bytes_async(
    cx: &AsyncApp,
    resource: crate::FileResource,
    bytes: Vec<u8>,
    options: WriteOptions,
    context: OperationContext,
) -> Task<ServiceResult<WriteReceipt>> {
    background_spawn(cx.background_executor(), move || {
        let mut cursor = std::io::Cursor::new(bytes);
        resource.save_from_reader(&mut cursor, options, &context)
    })
}

/// Stage an authorized resource as a consumer export on the background
/// executor. The returned lease must stay alive until the native consumer ends.
pub fn spawn_lease_export(
    cx: &App,
    app: gpui_app_data::AppData,
    resource: crate::FileResource,
    max_bytes: usize,
    context: OperationContext,
) -> Task<ServiceResult<ExportResourceLease>> {
    let executor = cx.background_executor();
    background_spawn(executor, move || {
        ExportResourceLease::lease_resource_for_export(&app, &resource, max_bytes, &context)
    })
}

/// Entity-owned scope which retains the GPUI release guard and file event
/// streams. Dropping it drops streams first, then disposes the UI owner.
pub struct FileEntityScope {
    subscriptions: Vec<EventSubscription<FileEvent>>,
    scope: gpui_system::gpui_adapter::EntityScope,
}

impl FileEntityScope {
    pub fn new<T: 'static>(owner: UiOwner, cx: &Context<T>) -> Self {
        Self {
            subscriptions: Vec::new(),
            scope: gpui_system::gpui_adapter::EntityScope::new(owner, cx),
        }
    }

    pub fn owner(&self) -> &UiOwner {
        self.scope.owner()
    }

    pub fn retain(&mut self, subscription: EventSubscription<FileEvent>) {
        self.subscriptions.push(subscription);
    }

    pub fn subscribe(&mut self, service: &FileService) {
        self.retain(service.subscribe());
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_prompt(
        options: PathPromptOptions,
        files: bool,
        directories: bool,
        multiple: bool,
        prompt: Option<&str>,
    ) {
        assert_eq!(options.files, files);
        assert_eq!(options.directories, directories);
        assert_eq!(options.multiple, multiple);
        assert_eq!(options.prompt.as_deref(), prompt);
    }

    #[test]
    fn open_file_options_convert_to_path_prompt_options() {
        let options = OpenFileOptions::new().with_prompt("Open a file");
        assert_prompt(
            path_prompt_options(&options),
            true,
            false,
            false,
            Some("Open a file"),
        );
        assert_prompt(
            PathPromptOptions::from(options),
            true,
            false,
            false,
            Some("Open a file"),
        );
        assert_prompt(
            path_prompt_options(OpenFileOptions::default()),
            true,
            false,
            false,
            None,
        );
    }

    #[test]
    fn open_files_options_convert_to_path_prompt_options() {
        let options = OpenFilesOptions::new().with_prompt("Open files");
        assert_prompt(
            path_prompt_options(&options),
            true,
            false,
            true,
            Some("Open files"),
        );
        assert_prompt(
            PathPromptOptions::from(options),
            true,
            false,
            true,
            Some("Open files"),
        );
        assert_prompt(
            path_prompt_options(OpenFilesOptions::default()),
            true,
            false,
            true,
            None,
        );
    }

    #[test]
    fn pick_directory_options_convert_to_path_prompt_options() {
        let options = PickDirectoryOptions::new().with_prompt("Pick a directory");
        assert_prompt(
            path_prompt_options(&options),
            false,
            true,
            false,
            Some("Pick a directory"),
        );
        assert_prompt(
            PathPromptOptions::from(options),
            false,
            true,
            false,
            Some("Pick a directory"),
        );
        assert_prompt(
            path_prompt_options(PickDirectoryOptions::default()),
            false,
            true,
            false,
            None,
        );
    }

    #[test]
    fn save_file_options_convert_to_path_prompt_options() {
        let options = SaveFileOptions::new().with_prompt("Save a file");
        assert_prompt(
            path_prompt_options(&options),
            true,
            false,
            false,
            Some("Save a file"),
        );
        assert_prompt(
            PathPromptOptions::from(options),
            true,
            false,
            false,
            Some("Save a file"),
        );
        assert_prompt(
            path_prompt_options(SaveFileOptions::default()),
            true,
            false,
            false,
            None,
        );
    }
}
