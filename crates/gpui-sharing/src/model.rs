use crate::{AllowedUrl, ErrorCode, SystemError, error};
use gpui_app_data::AppData;
use gpui_files::{ExportResourceLease, FileResource};
use gpui_system::{Completion, OperationContext, ServiceResult};
use std::{fmt, sync::Arc};

pub const MAX_SHARE_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_SHARE_ITEMS: usize = 32;

/// Sensitive text and URLs are deliberately absent from Debug output.
#[derive(Clone)]
pub enum ShareItem {
    Text(String),
    Url(AllowedUrl),
    Resource(ShareResource),
}
impl fmt::Debug for ShareItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Text(_) => "Text([redacted])",
            Self::Url(_) => "Url([redacted])",
            Self::Resource(_) => "Resource([redacted])",
        })
    }
}

/// A route selected before execution, never an implicit response to a native
/// denial, failure, cancellation, or unavailable share sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareRoute {
    Native,
    CopyText,
}

#[derive(Clone)]
pub struct ShareRequest {
    pub title: Option<String>,
    pub items: Vec<ShareItem>,
    pub route: ShareRoute,
}
impl fmt::Debug for ShareRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShareRequest")
            .field("items", &self.items)
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}
impl ShareRequest {
    pub fn native(item: ShareItem) -> Self {
        Self {
            title: None,
            items: vec![item],
            route: ShareRoute::Native,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), SystemError> {
        if self.items.is_empty() || self.items.len() > MAX_SHARE_ITEMS {
            return Err(error(
                ErrorCode::InvalidInput,
                "Share item count is invalid",
            ));
        }
        if self
            .title
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 256 || v.chars().any(char::is_control))
        {
            return Err(error(ErrorCode::InvalidInput, "Share title is invalid"));
        }
        let mut total = 0usize;
        for item in &self.items {
            let value = match item {
                ShareItem::Text(v) => v.as_str(),
                ShareItem::Url(v) => {
                    // AllowedUrl can be deserialized; revalidate at execution.
                    crate::UrlPolicy::new(["https", "http"])?.parse(v.as_str())?;
                    v.as_str()
                }
                ShareItem::Resource(resource) => {
                    if !resource.lease.is_active() || resource.lease.path().is_none() {
                        return Err(error(
                            ErrorCode::ResourceExpired,
                            "Share resource lease is expired",
                        ));
                    }
                    continue;
                }
            };
            if value.is_empty() || value.contains('\0') {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "Share payload is empty or contains a null character",
                ));
            }
            total = total.saturating_add(value.len());
        }
        if total > MAX_SHARE_TEXT_BYTES {
            return Err(error(
                ErrorCode::InvalidInput,
                "Share payload exceeds the size limit",
            ));
        }
        Ok(())
    }
    pub(crate) fn operations(&self) -> Vec<ShareOperation> {
        let mut operations = Vec::new();
        for item in &self.items {
            let operation = match item {
                ShareItem::Text(_) => ShareOperation::Text,
                ShareItem::Url(_) => ShareOperation::Url,
                ShareItem::Resource(_) => ShareOperation::Files,
            };
            if !operations.contains(&operation) {
                operations.push(operation);
            }
        }
        if self.items.len() > 1 {
            operations.push(ShareOperation::MultipleItems);
        }
        operations
    }
    pub(crate) fn copy_text(&self) -> Result<&str, SystemError> {
        match self.items.as_slice() {
            [ShareItem::Text(value)] => Ok(value),
            [ShareItem::Url(value)] => Ok(value.as_str()),
            _ => Err(error(
                ErrorCode::Unsupported,
                "Copy fallback requires exactly one text or URL item",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareOperation {
    Text,
    Url,
    Files,
    MultipleItems,
    TargetResult,
    CopyText,
}
impl ShareOperation {
    pub const ALL: [Self; 6] = [
        Self::Text,
        Self::Url,
        Self::Files,
        Self::MultipleItems,
        Self::TargetResult,
        Self::CopyText,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareReceipt {
    /// The OS accepted the request to invoke sharing UI. Neither visible
    /// presentation, target acceptance, nor eventual delivery was observed.
    SubmittedOutcomeUnobservable,
    /// An explicitly selected copy route published text to the OS clipboard.
    FallbackPerformed { kind: ShareFallbackKind },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareFallbackKind {
    CopyText,
}

/// An authorized file prepared on a background executor before opening a UI.
/// The immutable Arc lease pins the staged content and original authorization.
/// Native providers must clone and retain this value until their actual
/// consumer finishes or the provider's documented controlled retention ends;
/// dismissing a panel or returning a ShareReceipt is not a release signal.
#[derive(Clone)]
pub struct ShareResource {
    lease: Arc<ExportResourceLease>,
}
impl ShareResource {
    pub fn prepare(
        app: &AppData,
        resource: &FileResource,
        max_bytes: usize,
        context: &OperationContext,
    ) -> ServiceResult<Self> {
        ExportResourceLease::lease_resource_for_export(app, resource, max_bytes, context).map(
            |result| match result {
                Completion::Completed(lease) => Completion::Completed(Self {
                    lease: Arc::new(lease),
                }),
                Completion::Cancelled(reason) => Completion::Cancelled(reason),
            },
        )
    }
    /// For an explicitly installed native provider only. Never serialize this
    /// path or use it as a substitute for a picker authorization.
    pub fn export_lease(&self) -> &ExportResourceLease {
        &self.lease
    }
}
impl fmt::Debug for ShareResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ShareResource([redacted])")
    }
}
