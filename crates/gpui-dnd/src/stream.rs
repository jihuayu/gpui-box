//! Foreground transfer attestation, producer handoff leases and consumer stream
//! leases.
//!
//! Ownership is deliberately asymmetric:
//!
//! - a clipboard handoff's eager representations remain in the OS clipboard
//!   after the [`HandoffLease`] drops, as users expect;
//! - every delayed/lazy callback and every promised producer-side
//!   [`FileResource`] is revoked immediately on lease drop, explicit revoke,
//!   source exit, application termination or surface generation disposal;
//! - consumers read through an RAII [`PayloadStreamLease`] capped at
//!   [`MAX_ACTIVE_STREAM_LEASES`] service-wide.

use crate::error::{completed, expired, invalid};
use crate::format::{DeliveryMode, FormatDescriptor};
use crate::model::{
    HandoffId, MAX_ACTIVE_STREAM_LEASES, MAX_OFFERED_FORMATS, MAX_TEXT_BYTES,
    MAX_TRANSFERRED_FILES, TypedPayload,
};
use crate::service::ServiceInner;
use gpui_clipboard::{
    Clipboard, ClipboardInteraction, ClipboardProvider, ClipboardReceipt, ClipboardText,
};
use gpui_files::{FileResource, ResourceReader};
use gpui_system::{
    CancelReason, CancellationToken, Completion, OperationContext, ServiceResult, SurfaceId,
    SystemError,
};
use std::{
    fmt,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};

/// A service-issued attestation of a live foreground user gesture.
///
/// It is consumed by one clipboard write/read or drag start. It cannot be
/// transferred to a different service or paired with a foreign operation
/// context.
pub struct TransferInteraction {
    identity: Rc<()>,
    context: OperationContext,
    surface: Option<(SurfaceId, u64)>,
}

impl TransferInteraction {
    pub(crate) fn new(
        identity: Rc<()>,
        context: OperationContext,
        surface: Option<(SurfaceId, u64)>,
    ) -> Self {
        Self {
            identity,
            context,
            surface,
        }
    }

    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.context.cancellation = cancellation;
        self
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.context.deadline = Some(deadline);
        self
    }

    /// The explicit surface attestation, if the gesture was surface-scoped.
    pub fn surface(&self) -> Option<(SurfaceId, u64)> {
        self.surface
    }

    /// The live operation context carried by the gesture.
    pub fn context(&self) -> &OperationContext {
        &self.context
    }

    pub(crate) fn identity_matches(&self, identity: &Rc<()>) -> bool {
        Rc::ptr_eq(&self.identity, identity)
    }

    pub(crate) fn cancellation(&self) -> Option<CancelReason> {
        self.context.cancellation.reason()
    }
}

impl fmt::Debug for TransferInteraction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferInteraction")
            .field("surface", &self.surface)
            .field("has_ui", &self.context.ui.is_some())
            .field("deadline", &self.context.deadline.is_some())
            .finish()
    }
}

/// Where a clipboard handoff lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandoffScope {
    /// Lives until the OS replaces the clipboard or the service exits.
    Application,
    /// Bound to one live surface generation.
    Surface { surface: SurfaceId, generation: u64 },
}

impl HandoffScope {
    pub const fn application() -> Self {
        Self::Application
    }

    pub fn surface(surface: SurfaceId, generation: u64) -> Result<Self, SystemError> {
        if surface == SurfaceId(0) || generation == 0 {
            return Err(invalid(
                "A surface clipboard handoff requires a nonzero surface and generation",
            ));
        }
        Ok(Self::Surface {
            surface,
            generation,
        })
    }

    pub const fn surface_id(self) -> Option<SurfaceId> {
        match self {
            Self::Application => None,
            Self::Surface { surface, .. } => Some(surface),
        }
    }

    pub const fn generation(self) -> Option<u64> {
        match self {
            Self::Application => None,
            Self::Surface { generation, .. } => Some(generation),
        }
    }
}

/// Why delayed/lazy clipboard representations were revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandoffRevokeReason {
    LeaseDropped,
    Explicit,
    SourceExit,
    AppTerminating,
    SurfaceDisposed,
    Replaced,
    ClipboardCleared,
}

impl HandoffRevokeReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LeaseDropped => "lease_dropped",
            Self::Explicit => "explicit",
            Self::SourceExit => "source_exit",
            Self::AppTerminating => "app_terminating",
            Self::SurfaceDisposed => "surface_disposed",
            Self::Replaced => "replaced",
            Self::ClipboardCleared => "clipboard_cleared",
        }
    }
}

/// A lazy payload callback invoked outside all internal service borrows.
pub type LazyPayloadCallback = Rc<dyn Fn(&OperationContext) -> ServiceResult<TypedPayload>>;

/// One delayed representation advertised by a producer.
#[derive(Clone)]
pub struct LazyRepresentation {
    descriptor: FormatDescriptor,
    callback: LazyPayloadCallback,
    promised_files: Vec<FileResource>,
    estimated_bytes: Option<usize>,
}

/// Descriptive alias used by provider adapters.
pub type LazyPayloadProvider = LazyRepresentation;

impl LazyRepresentation {
    pub fn new(
        descriptor: FormatDescriptor,
        callback: impl Fn(&OperationContext) -> ServiceResult<TypedPayload> + 'static,
    ) -> Self {
        Self {
            descriptor,
            callback: Rc::new(callback),
            promised_files: Vec::new(),
            estimated_bytes: None,
        }
    }

    pub fn with_promised_files(
        mut self,
        files: impl IntoIterator<Item = FileResource>,
    ) -> Result<Self, SystemError> {
        let files = files.into_iter().collect::<Vec<_>>();
        if files.is_empty() || files.len() > MAX_TRANSFERRED_FILES {
            return Err(invalid(
                "A lazy representation promises 1 through 256 opaque resources",
            ));
        }
        self.promised_files = files;
        Ok(self)
    }

    pub const fn with_estimated_bytes(mut self, bytes: usize) -> Self {
        self.estimated_bytes = Some(bytes);
        self
    }

    pub fn descriptor(&self) -> &FormatDescriptor {
        &self.descriptor
    }

    pub const fn estimated_bytes(&self) -> Option<usize> {
        self.estimated_bytes
    }

    pub fn promised_files(&self) -> &[FileResource] {
        &self.promised_files
    }

    pub fn promised_file_ids(&self) -> Vec<gpui_files::ResourceId> {
        self.promised_files.iter().map(FileResource::id).collect()
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if self.promised_files.len() > MAX_TRANSFERRED_FILES {
            return Err(invalid(
                "A lazy representation promises at most 256 opaque resources",
            ));
        }
        Ok(())
    }

    pub(crate) fn invoke(&self, context: &OperationContext) -> ServiceResult<TypedPayload> {
        (self.callback)(context)
    }

    /// Release every promised producer-side resource immediately.
    pub fn release_promised_files(&self) {
        for resource in &self.promised_files {
            resource.release();
        }
    }
}

impl fmt::Debug for LazyRepresentation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LazyRepresentation")
            .field("descriptor", &self.descriptor)
            .field("estimated_bytes", &self.estimated_bytes)
            .field("promised_files", &self.promised_files.len())
            .finish()
    }
}

/// A producer's validated rich clipboard write.
#[derive(Clone, Debug)]
pub struct RichClipboardWrite {
    pub scope: HandoffScope,
    pub eager: Vec<TypedPayload>,
    pub delayed: Vec<LazyRepresentation>,
}

impl RichClipboardWrite {
    pub fn new(scope: HandoffScope) -> Self {
        Self {
            scope,
            eager: Vec::new(),
            delayed: Vec::new(),
        }
    }

    pub fn application() -> Self {
        Self::new(HandoffScope::Application)
    }

    pub fn surface(surface: SurfaceId, generation: u64) -> Result<Self, SystemError> {
        Ok(Self {
            scope: HandoffScope::surface(surface, generation)?,
            eager: Vec::new(),
            delayed: Vec::new(),
        })
    }

    pub fn with_eager(mut self, payload: TypedPayload) -> Result<Self, SystemError> {
        payload.validate()?;
        self.eager.push(payload);
        self.validate()?;
        Ok(self)
    }

    pub fn with_delayed(mut self, representation: LazyRepresentation) -> Result<Self, SystemError> {
        representation.validate()?;
        self.delayed.push(representation);
        self.validate()?;
        Ok(self)
    }

    pub fn formats(&self) -> Vec<FormatDescriptor> {
        self.eager
            .iter()
            .map(TypedPayload::descriptor)
            .chain(
                self.delayed
                    .iter()
                    .map(|delayed| delayed.descriptor().clone()),
            )
            .collect()
    }

    pub fn eager_formats(&self) -> Vec<FormatDescriptor> {
        self.eager.iter().map(TypedPayload::descriptor).collect()
    }

    pub fn delayed_formats(&self) -> Vec<FormatDescriptor> {
        self.delayed
            .iter()
            .map(|delayed| delayed.descriptor().clone())
            .collect()
    }

    pub fn has_delayed(&self) -> bool {
        !self.delayed.is_empty()
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        let total = self.eager.len() + self.delayed.len();
        if total == 0 {
            return Err(invalid(
                "A rich clipboard write requires at least one representation",
            ));
        }
        if total > MAX_OFFERED_FORMATS {
            return Err(invalid(
                "A rich clipboard write advertises too many formats",
            ));
        }
        let mut descriptors = Vec::with_capacity(total);
        for payload in &self.eager {
            payload.validate()?;
            let descriptor = payload.descriptor();
            if descriptors.iter().any(|existing| existing == &descriptor) {
                return Err(invalid(
                    "A rich clipboard write must not advertise duplicate formats",
                ));
            }
            descriptors.push(descriptor);
        }
        for representation in &self.delayed {
            representation.validate()?;
            let descriptor = representation.descriptor().clone();
            if descriptors.iter().any(|existing| existing == &descriptor) {
                return Err(invalid(
                    "A rich clipboard write must not advertise duplicate formats",
                ));
            }
            descriptors.push(descriptor);
        }
        Ok(())
    }

    /// Release all promised resources from delayed representations.
    pub fn release_promised_files(&self) {
        for representation in &self.delayed {
            representation.release_promised_files();
        }
    }
}

/// The OS accepted a clipboard offer. This is not proof of a paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffReceipt {
    pub handoff_id: HandoffId,
    /// Provider observation only; never permission to clear later.
    pub sequence: Option<u64>,
    pub eager_formats: Vec<FormatDescriptor>,
    pub delayed_formats: Vec<FormatDescriptor>,
}

impl HandoffReceipt {
    pub fn has_delayed(&self) -> bool {
        !self.delayed_formats.is_empty()
    }

    pub fn format_count(&self) -> usize {
        self.eager_formats.len() + self.delayed_formats.len()
    }
}

/// RAII ownership of one clipboard handoff's delayed callbacks.
///
/// Dropping this lease deliberately does **not** clear eager OS clipboard
/// formats. It does immediately revoke every delayed callback and release every
/// promised producer-side [`FileResource`].
pub struct HandoffLease {
    inner: Rc<ServiceInner>,
    handoff_id: HandoffId,
}

impl HandoffLease {
    pub(crate) fn new(inner: Rc<ServiceInner>, handoff_id: HandoffId) -> Self {
        Self { inner, handoff_id }
    }

    pub const fn id(&self) -> HandoffId {
        self.handoff_id
    }

    pub fn receipt(&self) -> Option<HandoffReceipt> {
        self.inner.handoff_receipt(self.handoff_id)
    }

    pub fn is_active(&self) -> bool {
        self.inner.handoff_is_active(self.handoff_id)
    }

    pub fn is_delayed_revoked(&self) -> bool {
        self.inner.handoff_delayed_revoked(self.handoff_id)
    }

    /// Revoke delayed callbacks and promised resources. Idempotent.
    pub fn revoke_delayed(&self) -> bool {
        self.inner
            .revoke_handoff_delayed(self.handoff_id, HandoffRevokeReason::Explicit)
    }

    /// Idempotent explicit revoke alias used by adapters.
    pub fn revoke(&self) -> bool {
        self.revoke_delayed()
    }
}

impl Drop for HandoffLease {
    fn drop(&mut self) {
        self.inner
            .revoke_handoff_delayed(self.handoff_id, HandoffRevokeReason::LeaseDropped);
    }
}

impl fmt::Debug for HandoffLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HandoffLease")
            .field("handoff_id", &self.handoff_id)
            .field("active", &self.is_active())
            .field("delayed_revoked", &self.is_delayed_revoked())
            .finish()
    }
}

/// Shared service-wide consumer stream budget.
pub(crate) struct StreamBudget {
    active: AtomicUsize,
}

impl StreamBudget {
    pub(crate) fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
        }
    }

    pub(crate) fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    fn acquire(&self) -> Result<(), SystemError> {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= MAX_ACTIVE_STREAM_LEASES {
                return Err(crate::error::busy(
                    "The maximum number of active payload stream leases is open",
                ));
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self) {
        // `release` is guarded by a one-shot AtomicBool on the lease, so the
        // counter can never underflow.
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A bounded consumer stream over one payload.
///
/// Reads require a background [`OperationContext`]; a UI-attached context is
/// rejected with `ErrorCode::InvalidInput`, matching `gpui-files`. Opening the
/// 65th simultaneous lease fails with `ErrorCode::Busy`. `release` and `Drop`
/// are idempotent and invalidate further reads.
pub struct PayloadStreamLease {
    bytes: Arc<[u8]>,
    cursor: usize,
    budget: Arc<StreamBudget>,
    revoked: Arc<AtomicBool>,
    released: AtomicBool,
}

impl PayloadStreamLease {
    fn open(
        bytes: Arc<[u8]>,
        budget: Arc<StreamBudget>,
        revoked: Arc<AtomicBool>,
    ) -> Result<Self, SystemError> {
        budget.acquire()?;
        Ok(Self {
            bytes,
            cursor: 0,
            budget,
            revoked,
            released: AtomicBool::new(false),
        })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn bytes_read(&self) -> usize {
        self.cursor
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.cursor)
    }

    pub fn is_released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    /// Idempotently release this lease and decrement the shared budget.
    pub fn release(&self) -> bool {
        let changed = !self.released.swap(true, Ordering::AcqRel);
        if changed {
            self.budget.release();
        }
        changed
    }

    fn check(&self, context: &OperationContext) -> Result<(), SystemError> {
        if self.is_released() || self.revoked.load(Ordering::Acquire) {
            return Err(expired("Payload stream lease is released or revoked"));
        }
        if context.ui.is_some() {
            return Err(invalid(
                "Payload streaming requires a background context without a UI attachment",
            ));
        }
        context.check_at(Instant::now())?;
        Ok(())
    }

    /// Read at most `max_bytes`. EOF is `Completed(None)`; cancellation is an
    /// ordinary outer `Completion::Cancelled`.
    pub fn read_chunk(
        &mut self,
        context: &OperationContext,
        max_bytes: usize,
    ) -> ServiceResult<Option<Vec<u8>>> {
        if max_bytes == 0 || max_bytes > 1024 * 1024 {
            return Err(invalid(
                "Payload chunk size must be between 1 byte and 1 MiB",
            ));
        }
        if let Some(cancelled) = crate::error::check_background(context)? {
            return Ok(cancelled);
        }
        self.check(context)?;
        if self.cursor >= self.bytes.len() {
            return completed(None);
        }
        let end = self.cursor.saturating_add(max_bytes).min(self.bytes.len());
        let chunk = self.bytes[self.cursor..end].to_vec();
        self.cursor = end;
        if self.is_released() || self.revoked.load(Ordering::Acquire) {
            return Err(expired("Payload stream lease was revoked during the read"));
        }
        if let Some(cancelled) = crate::error::check_background(context)? {
            return Ok(cancelled);
        }
        completed(Some(chunk))
    }

    /// Read the remaining payload with the default 64 KiB bound.
    pub fn read_to_end(&mut self, context: &OperationContext) -> ServiceResult<Vec<u8>> {
        let mut output = Vec::new();
        loop {
            match self.read_chunk(context, 64 * 1024)? {
                Completion::Completed(Some(chunk)) => output.extend_from_slice(&chunk),
                Completion::Completed(None) => return completed(output),
                Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
            }
        }
    }
}

impl Drop for PayloadStreamLease {
    fn drop(&mut self) {
        self.release();
    }
}

impl fmt::Debug for PayloadStreamLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PayloadStreamLease")
            .field("len", &self.len())
            .field("bytes_read", &self.bytes_read())
            .field("released", &self.is_released())
            .finish()
    }
}

/// A consumer's negotiated, owned payload.
#[derive(Clone)]
pub struct ConsumedPayload {
    descriptor: FormatDescriptor,
    delivery: DeliveryMode,
    payload: TypedPayload,
    budget: Arc<StreamBudget>,
    revoked: Arc<AtomicBool>,
}

impl ConsumedPayload {
    pub(crate) fn new(
        descriptor: FormatDescriptor,
        delivery: DeliveryMode,
        payload: TypedPayload,
        budget: Arc<StreamBudget>,
    ) -> Self {
        Self {
            descriptor,
            delivery,
            payload,
            budget,
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn descriptor(&self) -> &FormatDescriptor {
        &self.descriptor
    }

    pub const fn delivery(&self) -> DeliveryMode {
        self.delivery
    }

    pub fn payload(&self) -> &TypedPayload {
        &self.payload
    }

    pub fn into_payload(self) -> TypedPayload {
        self.payload
    }

    pub fn as_text(&self) -> Option<&str> {
        self.payload.as_text()
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.payload {
            TypedPayload::Png(bytes)
            | TypedPayload::Jpeg(bytes)
            | TypedPayload::Webp(bytes)
            | TypedPayload::Custom { bytes, .. } => Some(bytes),
            TypedPayload::Rgba8(image) => Some(image.bytes()),
            TypedPayload::PlainText(_)
            | TypedPayload::Html(_)
            | TypedPayload::Rtf(_)
            | TypedPayload::Markdown(_)
            | TypedPayload::Files(_) => None,
        }
    }

    pub fn rgba8(&self) -> Option<&crate::model::RgbaImageBuffer> {
        match &self.payload {
            TypedPayload::Rgba8(image) => Some(image),
            _ => None,
        }
    }

    /// Consumer-side opaque file handles. No path or URI is exposed.
    pub fn files(&self) -> Option<&[FileResource]> {
        self.payload.as_files()
    }

    pub fn file_resource(&self, index: usize) -> Option<FileResource> {
        self.files()?.get(index).cloned()
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }

    /// Open one bounded background stream lease. The service-wide cap is 64.
    pub fn open_stream(&self) -> Result<PayloadStreamLease, SystemError> {
        if self.is_revoked() {
            return Err(expired("The consumer payload grant was revoked"));
        }
        if self.payload.as_files().is_some() {
            return Err(crate::error::unsupported(
                "File payloads expose opaque readers instead of byte streams",
            ));
        }
        let bytes: Arc<[u8]> = match &self.payload {
            TypedPayload::PlainText(value)
            | TypedPayload::Html(value)
            | TypedPayload::Rtf(value)
            | TypedPayload::Markdown(value) => Arc::from(value.as_bytes()),
            TypedPayload::Png(bytes)
            | TypedPayload::Jpeg(bytes)
            | TypedPayload::Webp(bytes)
            | TypedPayload::Custom { bytes, .. } => Arc::from(bytes.as_slice()),
            TypedPayload::Rgba8(image) => Arc::clone(&image.bytes),
            TypedPayload::Files(_) => unreachable!("checked above"),
        };
        PayloadStreamLease::open(bytes, Arc::clone(&self.budget), Arc::clone(&self.revoked))
    }

    /// Open an opaque reader for one file handle. Requires a background context.
    pub fn open_file_reader(
        &self,
        index: usize,
        context: &OperationContext,
    ) -> Result<ResourceReader, SystemError> {
        match self.open_file_stream(index, context)? {
            Completion::Completed(reader) => Ok(reader),
            Completion::Cancelled(reason) => Err(crate::error::error(
                gpui_system::ErrorCode::ContextExpired,
                match reason {
                    CancelReason::User => "File streaming was cancelled by the user",
                    CancelReason::Caller => "File streaming was cancelled by the caller",
                    CancelReason::OwnerDisposed => "File streaming owner was disposed",
                    CancelReason::ProcessPolicy => "File streaming was cancelled by process policy",
                },
            )),
        }
    }

    pub fn open_file_stream(
        &self,
        index: usize,
        context: &OperationContext,
    ) -> ServiceResult<ResourceReader> {
        if self.is_revoked() {
            return Err(expired("The consumer payload grant was revoked"));
        }
        if context.ui.is_some() {
            return Err(invalid(
                "File streaming requires a background context without a UI attachment",
            ));
        }
        let resource = self
            .file_resource(index)
            .ok_or_else(|| invalid("File payload index is out of bounds"))?;
        resource.open_reader(context)
    }

    /// Explicitly invalidate this consumer grant and every lease it opened.
    pub fn revoke(&self) -> bool {
        !self.revoked.swap(true, Ordering::AcqRel)
    }
}

impl fmt::Debug for ConsumedPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConsumedPayload")
            .field("descriptor", &self.descriptor)
            .field("delivery", &self.delivery)
            .field("payload", &self.payload)
            .field("revoked", &self.is_revoked())
            .finish()
    }
}

/// The result of a rich clipboard read.
#[derive(Debug, Clone)]
pub enum ClipboardReadOutcome {
    /// The clipboard currently has no formats at all.
    Empty,
    /// Formats exist, but none matched the caller's preferences.
    NoMatchingFormat {
        available_offers: Vec<crate::format::PayloadOffer>,
    },
    Matched(ConsumedPayload),
}

impl ClipboardReadOutcome {
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn matched(&self) -> Option<&ConsumedPayload> {
        match self {
            Self::Matched(payload) => Some(payload),
            _ => None,
        }
    }
}

fn validate_plain_text(text: &str) -> Result<(), SystemError> {
    if text.len() > MAX_TEXT_BYTES || text.contains('\0') {
        return Err(invalid(
            "Clipboard plain text exceeds one MiB or contains NUL",
        ));
    }
    Ok(())
}

fn interaction_with(
    clipboard: &Clipboard,
    cancellation: Option<CancellationToken>,
    deadline: Option<Instant>,
) -> Result<ClipboardInteraction, SystemError> {
    let mut interaction = clipboard.user_initiated()?;
    if let Some(cancellation) = cancellation {
        interaction = interaction.with_cancellation(cancellation);
    }
    if let Some(deadline) = deadline {
        interaction = interaction.with_deadline(deadline);
    }
    Ok(interaction)
}

/// Write plain text through the untouched `#25` [`Clipboard`] contract.
///
/// This validates the same one-MiB/NUL bounds and returns the same
/// [`ClipboardReceipt`] as a direct `gpui-clipboard` write.
pub fn write_plain_text_through(
    clipboard: &Clipboard,
    text: &str,
) -> ServiceResult<ClipboardReceipt> {
    let interaction = interaction_with(clipboard, None, None)?;
    // `Clipboard::write_text` performs the authoritative gesture check before
    // validating text, so the bridge preserves #25's exact error ordering.
    clipboard.write_text(text, interaction)
}

/// Cancellation/deadline-aware `#25` plain-text bridge write.
pub fn write_plain_text_through_with(
    clipboard: &Clipboard,
    text: &str,
    cancellation: CancellationToken,
    deadline: Option<Instant>,
) -> ServiceResult<ClipboardReceipt> {
    let interaction = interaction_with(clipboard, Some(cancellation), deadline)?;
    clipboard.write_text(text, interaction)
}

/// Read plain text through the untouched `#25` [`Clipboard`] contract.
pub fn read_plain_text_through(clipboard: &Clipboard) -> ServiceResult<ClipboardText> {
    let interaction = clipboard.user_initiated()?;
    clipboard.read_text(interaction)
}

/// Cancellation/deadline-aware `#25` plain-text bridge read.
pub fn read_plain_text_through_with(
    clipboard: &Clipboard,
    cancellation: CancellationToken,
    deadline: Option<Instant>,
) -> ServiceResult<ClipboardText> {
    let interaction = interaction_with(clipboard, Some(cancellation), deadline)?;
    clipboard.read_text(interaction)
}

/// Direct provider bridge used by hosts that already own an append-only plain
/// text lane and want to prove the same contract without a [`Clipboard`].
pub fn write_plain_text_via_provider(
    provider: &dyn ClipboardProvider,
    text: &str,
    context: &OperationContext,
) -> ServiceResult<ClipboardReceipt> {
    context.require_ui()?;
    context.check_at(Instant::now())?;
    if let Some(reason) = context.cancellation.reason() {
        return Ok(Completion::Cancelled(reason));
    }
    validate_plain_text(text)?;
    provider.write_text(text, context)
}

/// Direct provider bridge read.
pub fn read_plain_text_via_provider(
    provider: &dyn ClipboardProvider,
    context: &OperationContext,
) -> ServiceResult<ClipboardText> {
    context.require_ui()?;
    context.check_at(Instant::now())?;
    if let Some(reason) = context.cancellation.reason() {
        return Ok(Completion::Cancelled(reason));
    }
    let result = provider.read_text(context)?;
    if let Completion::Completed(ClipboardText::Text(text)) = &result {
        validate_plain_text(text)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RgbaImageBuffer;

    fn background() -> OperationContext {
        OperationContext::default()
    }

    #[test]
    fn stream_budget_caps_at_sixty_four_and_release_is_idempotent() {
        let budget = Arc::new(StreamBudget::new());
        let revoked = Arc::new(AtomicBool::new(false));
        let mut leases = Vec::new();
        for _ in 0..MAX_ACTIVE_STREAM_LEASES {
            leases.push(
                PayloadStreamLease::open(
                    Arc::from(b"hello".as_slice()),
                    Arc::clone(&budget),
                    Arc::clone(&revoked),
                )
                .expect("under cap"),
            );
        }
        assert_eq!(budget.active(), MAX_ACTIVE_STREAM_LEASES);
        let error = PayloadStreamLease::open(
            Arc::from(b"hello".as_slice()),
            Arc::clone(&budget),
            Arc::clone(&revoked),
        )
        .expect_err("65th lease must fail");
        assert_eq!(error.code, gpui_system::ErrorCode::Busy);
        let mut lease = leases.pop().expect("lease");
        assert!(lease.release());
        assert!(!lease.release());
        assert_eq!(budget.active(), MAX_ACTIVE_STREAM_LEASES - 1);
        assert!(lease.read_chunk(&background(), 1).is_err());
        drop(leases);
        assert_eq!(budget.active(), 0);
    }

    #[test]
    fn stream_reads_are_bounded_cancellable_and_background_only() {
        let budget = Arc::new(StreamBudget::new());
        let consumed = ConsumedPayload::new(
            FormatDescriptor::plain_text(),
            DeliveryMode::Eager,
            TypedPayload::plain_text("hello world").expect("text"),
            budget,
        );
        let owner = gpui_system::UiOwner::new(SurfaceId(90));
        let ui = OperationContext {
            ui: Some(owner.context(None).expect("ui")),
            ..OperationContext::default()
        };
        let mut lease = consumed.open_stream().expect("lease");
        assert_eq!(
            lease.read_chunk(&ui, 4).expect_err("ui context").code,
            gpui_system::ErrorCode::InvalidInput
        );
        let mut first = lease.read_chunk(&background(), 5).expect("read");
        assert_eq!(first, Completion::Completed(Some(b"hello".to_vec())));
        first = lease.read_chunk(&background(), 64).expect("read");
        assert_eq!(first, Completion::Completed(Some(b" world".to_vec())));
        assert_eq!(
            lease.read_chunk(&background(), 1).expect("eof"),
            Completion::Completed(None)
        );
        assert!(consumed.revoke());
        assert!(lease.read_chunk(&background(), 1).is_err());
    }

    #[test]
    fn consumed_files_expose_no_bytes_and_open_opaque_readers() {
        let provider = gpui_files::FakeFileProvider::new();
        let resource = provider.create_stream(
            "report.txt",
            b"report",
            gpui_files::DurableGrantSupport::Unsupported,
        );
        let consumed = ConsumedPayload::new(
            FormatDescriptor::file_resources(),
            DeliveryMode::Eager,
            TypedPayload::files([resource]).expect("files"),
            Arc::new(StreamBudget::new()),
        );
        assert!(consumed.open_stream().is_err());
        assert_eq!(consumed.files().expect("files").len(), 1);
        let error = match consumed.open_file_stream(99, &background()) {
            Err(error) => error,
            Ok(_) => panic!("out-of-range file index should fail"),
        };
        assert_eq!(error.code, gpui_system::ErrorCode::InvalidInput);
        let mut reader = match consumed.open_file_stream(0, &background()).expect("reader") {
            Completion::Completed(reader) => reader,
            Completion::Cancelled(reason) => panic!("unexpected cancel: {reason:?}"),
        };
        let chunk = reader.read_chunk(&background(), 64).expect("chunk");
        assert_eq!(chunk, Completion::Completed(Some(b"report".to_vec())));
    }

    #[test]
    fn rgba_payload_streams_exact_bytes_and_debug_is_bounded() {
        let image = RgbaImageBuffer::new(1, 1, vec![1, 2, 3, 4]).expect("image");
        let consumed = ConsumedPayload::new(
            FormatDescriptor::rgba8(),
            DeliveryMode::Eager,
            TypedPayload::Rgba8(image),
            Arc::new(StreamBudget::new()),
        );
        let mut lease = consumed.open_stream().expect("lease");
        let bytes = match lease.read_to_end(&background()).expect("read") {
            Completion::Completed(bytes) => bytes,
            Completion::Cancelled(reason) => panic!("unexpected cancel: {reason:?}"),
        };
        assert_eq!(bytes, vec![1, 2, 3, 4]);
        assert!(!format!("{consumed:?}").contains("1, 2, 3, 4"));
    }
}
