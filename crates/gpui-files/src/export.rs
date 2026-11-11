//! Consumer export leases pinned by `gpui-app-data`.

use crate::error::{DEFAULT_CHUNK_SIZE, check_background, completed, invalid_input, io_error};
use crate::resource::FileResource;
use gpui_app_data::{AppData, ExportLease};
use gpui_system::{Completion, OperationContext, ServiceResult};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

/// A staged export that stays pinned until explicit release or drop.
///
/// The inner [`ExportLease`] keeps the file out of
/// `AppData::cleanup_exports(Duration::ZERO, ...)`. The optional resource clone
/// keeps an authorized source handle alive for the consumer lifetime. `Debug`
/// never prints the export path.
pub struct ExportResourceLease {
    lease: Option<ExportLease>,
    resource: Option<FileResource>,
    bytes: u64,
}

impl ExportResourceLease {
    /// Stage in-memory bytes through `AppData` and retain the export pin.
    pub fn stage_export_bytes(
        app: &AppData,
        bytes: &[u8],
        max_bytes: usize,
        context: &OperationContext,
    ) -> ServiceResult<Self> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        if bytes.len() > max_bytes {
            return Err(invalid_input("Export size limit exceeded"));
        }
        let lease = match app.temporary_export(bytes, max_bytes, context)? {
            Completion::Completed(lease) => lease,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        completed(Self {
            lease: Some(lease),
            resource: None,
            bytes: bytes.len() as u64,
        })
    }

    /// Stream a caller-provided reader directly into a pinned `AppData` export.
    /// The pin exists before the first chunk is written, so a concurrent
    /// cleanup cannot remove the in-progress file.
    pub fn stage_export_from_reader(
        app: &AppData,
        reader: &mut dyn Read,
        max_bytes: usize,
        context: &OperationContext,
    ) -> ServiceResult<Self> {
        Self::stage_stream(app, max_bytes, context, |context, chunk_bytes| {
            if let Some(cancelled) = check_background(context)? {
                return Ok(cancelled);
            }
            let mut buffer = vec![0; chunk_bytes];
            let read = reader.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                return completed(None);
            }
            buffer.truncate(read);
            completed(Some(buffer))
        })
    }

    /// Read an authorized resource into a pinned export and retain the opaque
    /// resource handle for the lifetime of the consumer lease.
    pub fn lease_resource_for_export(
        app: &AppData,
        resource: &FileResource,
        max_bytes: usize,
        context: &OperationContext,
    ) -> ServiceResult<Self> {
        let mut reader = match resource.open_reader(context)? {
            Completion::Completed(reader) => reader,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        let mut lease =
            match Self::stage_stream(app, max_bytes, context, |context, chunk_bytes| {
                reader.read_chunk(context, chunk_bytes)
            })? {
                Completion::Completed(lease) => lease,
                Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
            };
        lease.resource = Some(resource.clone());
        completed(lease)
    }

    fn stage_stream(
        app: &AppData,
        max_bytes: usize,
        context: &OperationContext,
        mut next: impl FnMut(&OperationContext, usize) -> ServiceResult<Option<Vec<u8>>>,
    ) -> ServiceResult<Self> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let lease = match app.temporary_export(&[], max_bytes, context)? {
            Completion::Completed(lease) => lease,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        let path = lease.path().to_path_buf();
        let result = write_export(&path, max_bytes, context, &mut next);
        match result {
            Ok(Completion::Completed(bytes)) => completed(Self {
                lease: Some(lease),
                resource: None,
                bytes,
            }),
            Ok(Completion::Cancelled(reason)) => {
                drop(lease);
                let _ = fs::remove_file(&path);
                Ok(Completion::Cancelled(reason))
            }
            Err(error) => {
                drop(lease);
                let _ = fs::remove_file(&path);
                Err(error)
            }
        }
    }

    /// The staged export path for native consumer handoff. It is not part of
    /// `Debug` output.
    pub fn path(&self) -> Option<&Path> {
        self.lease.as_ref().map(ExportLease::path)
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn resource_id(&self) -> Option<crate::ResourceId> {
        self.resource.as_ref().map(FileResource::id)
    }

    pub fn is_active(&self) -> bool {
        self.lease.is_some()
    }

    /// Release the pin early. Idempotent; it never deletes the export itself.
    pub fn release(&mut self) -> bool {
        let released = self.lease.take().is_some();
        self.resource.take();
        released
    }
}

fn write_export(
    path: &Path,
    max_bytes: usize,
    context: &OperationContext,
    next: &mut impl FnMut(&OperationContext, usize) -> ServiceResult<Option<Vec<u8>>>,
) -> ServiceResult<u64> {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(io_error)?;
    let mut written = 0u64;
    loop {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        match next(context, DEFAULT_CHUNK_SIZE)? {
            Completion::Completed(Some(bytes)) => {
                let total = written.saturating_add(bytes.len() as u64);
                if total > max_bytes as u64 {
                    return Err(invalid_input("Export size limit exceeded"));
                }
                file.write_all(&bytes).map_err(io_error)?;
                written = total;
                if let Some(cancelled) = check_background(context)? {
                    return Ok(cancelled);
                }
            }
            Completion::Completed(None) => break,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        }
    }
    file.flush().map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    if let Some(cancelled) = check_background(context)? {
        return Ok(cancelled);
    }
    completed(written)
}

impl fmt::Debug for ExportResourceLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExportResourceLease")
            .field("active", &self.lease.is_some())
            .field("bytes", &self.bytes)
            .field("resource", &self.resource)
            .finish()
    }
}
