#![cfg(feature = "dnd")]

//! End-to-end rich clipboard and drag-and-drop contract tests through the
//! `gpui-box` facade. No native clipboard, drag session or filesystem path is
//! touched: every test uses the deterministic fakes.

use gpui_box::dnd::*;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation succeeded") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

fn next_payload<T>(subscription: &EventSubscription<T>) -> Option<Event<T>> {
    let mut next = subscription.try_next().ok()?;
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(&mut next).poll(&mut context) {
        Poll::Ready(Some(EventEnvelope { payload, .. })) => Some(payload),
        Poll::Ready(None) | Poll::Pending => None,
    }
}

fn background() -> OperationContext {
    OperationContext::default()
}

struct Setup {
    fake: FakeRichTransferProvider,
    service: RichTransferService,
    owner: UiOwner,
    ui: UiContext,
}

fn setup(surface: u64) -> Setup {
    let fake = FakeRichTransferProvider::new();
    let service = RichTransferService::from_fake(fake.clone());
    let owner = UiOwner::new(SurfaceId(surface));
    let ui = owner.context(None).expect("ui context");
    service
        .attach_surface(SurfaceId(surface), 1, true)
        .expect("surface");
    Setup {
        fake,
        service,
        owner,
        ui,
    }
}

fn surface(setup: &Setup) -> SurfaceId {
    setup.ui.surface
}

fn interaction(setup: &Setup) -> TransferInteraction {
    setup
        .service
        .user_initiated_with(setup.ui.clone())
        .expect("gesture")
}

fn rect() -> LogicalRect {
    LogicalRect::new(0, 0, 1000, 1000).expect("rect")
}

fn point(x: i64, y: i64) -> DragPoint {
    DragPoint::from_milli(x, y)
}

fn preview() -> DragPreview {
    DragPreview::new("drag preview").expect("preview")
}

fn eager_text(value: &str) -> TypedPayload {
    TypedPayload::plain_text(value).expect("plain text")
}

#[derive(Default)]
struct MemoryClipboardState {
    formats_present: bool,
    text: Option<String>,
    sequence: u32,
    writes: usize,
}

#[derive(Clone, Default)]
struct MemoryClipboard {
    state: Rc<RefCell<MemoryClipboardState>>,
}

impl MemoryClipboard {
    fn with_non_text_format() -> Self {
        let clipboard = Self::default();
        clipboard.state.borrow_mut().formats_present = true;
        clipboard
    }

    fn write_count(&self) -> usize {
        self.state.borrow().writes
    }
}

impl ClipboardProvider for MemoryClipboard {
    fn capabilities(&self) -> Vec<Capability<ClipboardOperation>> {
        [ClipboardOperation::ReadText, ClipboardOperation::WriteText]
            .into_iter()
            .map(|operation| Capability {
                operation,
                support: Support::Native,
                availability: Availability::Available,
                configuration: Configuration::Ready,
                permission: PermissionState::NotRequired,
                evidence: Evidence::ContractOnly,
                constraints: Vec::new(),
                observed_revision: 1,
            })
            .collect()
    }

    fn read_text(&self, _: &OperationContext) -> ServiceResult<ClipboardText> {
        let state = self.state.borrow();
        if !state.formats_present {
            return Ok(Completion::Completed(ClipboardText::Empty));
        }
        Ok(Completion::Completed(match &state.text {
            Some(text) => ClipboardText::Text(text.clone()),
            None => ClipboardText::NoText,
        }))
    }

    fn write_text(&self, text: &str, _: &OperationContext) -> ServiceResult<ClipboardReceipt> {
        let mut state = self.state.borrow_mut();
        state.formats_present = true;
        state.text = Some(text.to_owned());
        state.writes = state.writes.saturating_add(1);
        state.sequence = state.sequence.saturating_add(1);
        Ok(Completion::Completed(ClipboardReceipt {
            sequence: Some(state.sequence),
        }))
    }
}

fn new_clipboard(provider: MemoryClipboard, ui: UiContext) -> Clipboard {
    Clipboard::new(Rc::new(provider), ui).expect("clipboard")
}

#[test]
fn facade_constants_matrix_and_canonical_format_table() {
    assert_eq!(UPSTREAM_GPUI_REVISION, gpui_box::UPSTREAM_REVISION);
    assert_eq!(PACKAGE_VERSION, gpui_box::VERSION);
    assert_eq!(DndOperation::ALL.len(), 6);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    assert_eq!(DEFAULT_DND_EVENT_CAPACITY, 64);
    assert_eq!(MAX_ACTIVE_STREAM_LEASES, 64);
    assert_eq!(MAX_OFFERED_FORMATS, 32);
    assert_eq!(MAX_TEXT_BYTES, gpui_box::clipboard::MAX_TEXT_BYTES);

    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 36);
    let cells = matrix
        .iter()
        .map(|row| (row.platform, row.operation))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(cells.len(), 36);
    for row in &matrix {
        assert_eq!(row.evidence, Evidence::ContractOnly);
        assert!(!row.required_host_configuration.is_empty());
        assert!(!row.notes.is_empty());
        assert!(row.has_link("#52"));
    }
    for platform in PlatformTarget::ALL {
        assert_eq!(platform.operations().len(), 6);
        for operation in DndOperation::ALL {
            assert_eq!(
                operation_support(platform, operation).support,
                platform.support(operation)
            );
        }
    }
    assert_eq!(
        PlatformTarget::Windows.support(DndOperation::TransferFileResources),
        Support::Native
    );
    assert_eq!(
        PlatformTarget::Linux.support(DndOperation::ProvideDelayedPayload),
        Support::Conditional
    );
    assert_eq!(
        PlatformTarget::Android.support(DndOperation::ProvideDelayedPayload),
        Support::Conditional
    );
    assert_eq!(
        PlatformTarget::Ios.support(DndOperation::ReadRichClipboard),
        Support::Native
    );
    assert_eq!(
        PlatformTarget::Web.support(DndOperation::ProvideDelayedPayload),
        Support::Unsupported
    );
    assert_eq!(
        PlatformTarget::Web.support(DndOperation::ReadRichClipboard),
        Support::Conditional
    );

    let formats = standard_formats();
    assert_eq!(formats.len(), 9);
    assert_eq!(
        FormatDescriptor::plain_text().mime.as_str(),
        "text/plain;charset=utf-8"
    );
    assert_eq!(
        FormatDescriptor::plain_text().uti.as_str(),
        "public.utf8-plain-text"
    );
    assert_eq!(FormatDescriptor::html().class, FormatClass::RichText);
    assert_eq!(FormatDescriptor::png().class, FormatClass::Image);
    assert_eq!(FormatDescriptor::rgba8().class, FormatClass::Image);
    assert_eq!(FormatDescriptor::file_resources().class, FormatClass::Files);
}

#[test]
fn mime_uti_validation_negotiation_and_payload_bounds() {
    let canonical = MimeType::parse("TEXT/Plain;Charset=UTF-8").expect("mime");
    assert_eq!(canonical.as_str(), "text/plain;charset=utf-8");
    assert_eq!(canonical.essence(), "text/plain");
    assert_eq!(canonical.media_type(), "text");
    assert_eq!(canonical.subtype(), "plain");
    assert_eq!(canonical.parameter("charset"), Some("utf-8"));
    assert!(canonical.matches_essence(&MimeType::parse("text/plain").expect("bare")));

    for bad in [
        "",
        "text",
        "text/plain/extra",
        "text /plain",
        "text/plain;charset",
        "text/plain;charset=utf-8;charset=latin1",
        "text/plain;bad==value",
        "text/plain;\"unterminated",
        "text/plain;ü=1",
    ] {
        assert!(MimeType::parse(bad).is_err(), "{bad:?}");
    }

    let uti = UtiIdentifier::parse("Public.PNG").expect("uti");
    assert_eq!(uti.as_str(), "public.png");
    for bad in [
        "",
        "public",
        ".public",
        "public.",
        "public..png",
        "public png",
    ] {
        assert!(UtiIdentifier::parse(bad).is_err(), "{bad:?}");
    }

    let text = FormatDescriptor::plain_text();
    let html = FormatDescriptor::html();
    let offers = [
        PayloadOffer::new(html.clone(), DeliveryMode::Eager),
        PayloadOffer::new(text.clone(), DeliveryMode::Delayed),
        PayloadOffer::new(text.clone(), DeliveryMode::Eager),
    ];
    let negotiated = negotiate_format(&offers, std::slice::from_ref(&text)).expect("match");
    assert_eq!(negotiated.offer_index, 2);
    assert_eq!(negotiated.delivery, DeliveryMode::Eager);
    assert_eq!(negotiate_format(&offers, &[FormatDescriptor::png()]), None);

    let image = RgbaImageBuffer::new(2, 2, vec![9; 16]).expect("rgba");
    let payload = TypedPayload::rgba8(image).expect("payload");
    let debug = format!("{payload:?}");
    assert!(debug.contains("bytes"));
    assert!(!debug.contains("9, 9, 9, 9"));
    assert!(RgbaImageBuffer::new(0, 1, vec![0; 4]).is_err());
    assert!(RgbaImageBuffer::new(2, 2, vec![0; 15]).is_err());
    assert!(TypedPayload::plain_text("a\0b").is_err());
    assert!(TypedPayload::png(Vec::new()).is_err());
    assert!(TypedPayload::custom(FormatDescriptor::plain_text(), b"x").is_err());
}

#[test]
fn plain_text_bridge_preserves_25_clipboard_semantics() {
    let owner = UiOwner::new(SurfaceId(900));
    let ui = owner.context(None).expect("ui");
    let provider = MemoryClipboard::default();
    let clipboard = new_clipboard(provider.clone(), ui);

    assert_eq!(
        completed(read_plain_text_through(&clipboard)),
        ClipboardText::Empty
    );
    let receipt = completed(write_plain_text_through(&clipboard, "hello 中文 🧑🏽‍💻"));
    assert_eq!(receipt.sequence, Some(1));
    assert_eq!(
        completed(read_plain_text_through(&clipboard)),
        ClipboardText::Text("hello 中文 🧑🏽‍💻".to_owned())
    );

    completed(write_plain_text_through(&clipboard, ""));
    assert_eq!(
        completed(read_plain_text_through(&clipboard)),
        ClipboardText::Text(String::new())
    );

    let non_text = MemoryClipboard::with_non_text_format();
    let non_text = new_clipboard(non_text, owner.context(None).expect("ui"));
    assert_eq!(
        completed(read_plain_text_through(&non_text)),
        ClipboardText::NoText
    );

    let error =
        write_plain_text_through(&clipboard, "embedded\0nul").expect_err("NUL must be rejected");
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let oversized = "x".repeat(MAX_TEXT_BYTES + 1);
    let error = write_plain_text_through(&clipboard, &oversized).expect_err("oversized text");
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(provider.write_count(), 2);

    let cancellation = CancellationToken::default();
    cancellation.cancel(CancelReason::User);
    match write_plain_text_through_with(&clipboard, "cancelled", cancellation, None)
        .expect("cancelled result")
    {
        Completion::Cancelled(CancelReason::User) => {}
        other => panic!("unexpected result: {other:?}"),
    }
    assert_eq!(provider.write_count(), 2, "cancellation cannot write");

    let deadline = Instant::now() - Duration::from_secs(1);
    let error = write_plain_text_through_with(
        &clipboard,
        "late",
        CancellationToken::default(),
        Some(deadline),
    )
    .expect_err("elapsed deadline");
    assert_eq!(error.code, ErrorCode::TimedOut);
    assert_eq!(provider.write_count(), 2);
}

#[test]
fn rich_clipboard_eager_persistence_and_immediate_delayed_revocation() {
    let setup = crate::setup(1);
    let surface_id = surface(&setup);
    let calls = Rc::new(Cell::new(0u32));
    let captured = calls.clone();
    let delayed = LazyRepresentation::new(FormatDescriptor::html(), move |_| {
        captured.set(captured.get().saturating_add(1));
        Ok(Completion::Completed(
            TypedPayload::html("<b>lazy</b>").expect("html"),
        ))
    });
    let write = RichClipboardWrite::surface(surface_id, 1)
        .expect("scope")
        .with_eager(eager_text("eager"))
        .expect("eager")
        .with_delayed(delayed)
        .expect("delayed");
    let (receipt, lease) = completed(
        setup
            .service
            .write_clipboard_receipt(interaction(&setup), write),
    );
    assert_eq!(receipt.eager_formats, vec![FormatDescriptor::plain_text()]);
    assert_eq!(receipt.delayed_formats, vec![FormatDescriptor::html()]);
    assert!(lease.is_active());
    assert!(!lease.is_delayed_revoked());
    assert_eq!(setup.fake.publish_call_count(), 1);
    assert_eq!(setup.fake.clipboard_entries().len(), 2);

    let matched = completed(setup.service.read_clipboard(
        interaction(&setup),
        &[FormatDescriptor::html(), FormatDescriptor::plain_text()],
    ));
    let payload = matched.matched().expect("delayed payload");
    assert_eq!(payload.delivery(), DeliveryMode::Delayed);
    assert_eq!(payload.as_text(), Some("<b>lazy</b>"));
    assert_eq!(calls.get(), 1);
    let _ = completed(
        setup
            .service
            .read_clipboard(interaction(&setup), &[FormatDescriptor::html()]),
    );
    assert_eq!(calls.get(), 2);

    drop(lease);
    assert_eq!(setup.fake.released_delayed_tokens().len(), 1);
    assert_eq!(
        setup.fake.clipboard_entries().len(),
        1,
        "eager representation remains hosted"
    );
    assert!(matches!(
        setup.fake.clipboard_entries()[0],
        ClipboardEntry::Eager(_)
    ));
    let outcome = completed(
        setup
            .service
            .read_clipboard(interaction(&setup), &[FormatDescriptor::plain_text()]),
    );
    assert_eq!(
        outcome.matched().and_then(ConsumedPayload::as_text),
        Some("eager")
    );
    let outcome = completed(
        setup
            .service
            .read_clipboard(interaction(&setup), &[FormatDescriptor::html()]),
    );
    assert!(matches!(
        outcome,
        ClipboardReadOutcome::NoMatchingFormat { .. }
    ));
    assert_eq!(calls.get(), 2, "revoked callback is never invoked");

    // Promised file handles are released immediately with the lease.
    let files = FakeFileProvider::new();
    let promised = files.create_read_only_stream(
        "promised.bin",
        b"promised",
        DurableGrantSupport::Unsupported,
    );
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), move |_| {
        Ok(Completion::Completed(TypedPayload::files([files
            .create_stream(
                "late.bin",
                b"late",
                DurableGrantSupport::Unsupported,
            )])?))
    })
    .with_promised_files([promised.clone()])
    .expect("promised files");
    let write = RichClipboardWrite::application()
        .with_delayed(delayed)
        .expect("write");
    let lease = completed(setup.service.write_clipboard(interaction(&setup), write));
    assert!(!promised.is_released());
    assert!(lease.revoke_delayed());
    assert!(!lease.revoke_delayed());
    assert!(promised.is_released());
}

#[test]
fn surface_generation_advance_revokes_handoff_and_mid_handoff_toctou_is_clean() {
    let setup = crate::setup(2);
    let surface_id = surface(&setup);
    let promised = FakeFileProvider::new().create_stream(
        "promised.txt",
        b"x",
        DurableGrantSupport::Unsupported,
    );
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), |_| {
        Ok(Completion::Completed(TypedPayload::files([])?))
    })
    .with_promised_files([promised.clone()])
    .expect("promised");
    let write = RichClipboardWrite::surface(surface_id, 1)
        .expect("scope")
        .with_eager(eager_text("surface eager"))
        .expect("eager")
        .with_delayed(delayed)
        .expect("delayed");
    let lease = completed(setup.service.write_clipboard(interaction(&setup), write));
    assert!(!promised.is_released());

    setup.owner.advance_generation().expect("advance");
    setup
        .service
        .attach_surface(surface_id, 2, true)
        .expect("attach");
    assert!(promised.is_released());
    assert!(!lease.is_active());
    assert!(lease.receipt().is_none());
    assert_eq!(setup.fake.released_delayed_tokens().len(), 1);
    assert!(
        setup
            .service
            .user_initiated_with(setup.owner.context(None).expect("new ui"))
            .is_ok()
    );
    let stale = setup
        .service
        .user_initiated_for_surface(surface_id, 1)
        .expect_err("old generation is stale");
    assert_eq!(stale.code, ErrorCode::ContextExpired);

    // A provider hook disposes the surface generation while publication is in
    // flight. The service must revoke the handoff, release promises and return a
    // typed context error without leaving a live lease.
    let fake = FakeRichTransferProvider::new();
    let service = RichTransferService::from_fake(fake.clone());
    let owner = UiOwner::new(SurfaceId(3));
    let ui = owner.context(None).expect("ui");
    service
        .attach_surface(SurfaceId(3), 1, true)
        .expect("surface");
    let late = FakeFileProvider::new().create_stream(
        "late.txt",
        b"late",
        DurableGrantSupport::Unsupported,
    );
    let captured_late = late.clone();
    let service_hook = service.clone();
    fake.set_on_publish(Some(Rc::new(move |_| {
        service_hook.dispose_surface_generation(SurfaceId(3), 1);
        assert!(captured_late.is_released());
    })));
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), |_| {
        Ok(Completion::Completed(TypedPayload::files([])?))
    })
    .with_promised_files([late.clone()])
    .expect("promised");
    let write = RichClipboardWrite::surface(SurfaceId(3), 1)
        .expect("scope")
        .with_delayed(delayed)
        .expect("write");
    let interaction = service.user_initiated_with(ui).expect("gesture");
    let error = service
        .write_clipboard(interaction, write)
        .expect_err("mid-handoff invalidation");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert!(late.is_released());
    assert_eq!(fake.released_delayed_tokens().len(), 1);
}

#[test]
fn consumer_stream_leases_are_capped_bounded_and_background_only() {
    let setup = crate::setup(4);
    setup
        .fake
        .set_external_plain_text("hello stream")
        .expect("text");
    let outcome = completed(
        setup
            .service
            .read_clipboard(interaction(&setup), &[FormatDescriptor::plain_text()]),
    );
    let payload = outcome.matched().expect("payload").clone();
    assert_eq!(payload.delivery(), DeliveryMode::Eager);
    assert_eq!(payload.as_text(), Some("hello stream"));

    let ui_context = OperationContext {
        ui: Some(setup.ui.clone()),
        ..OperationContext::default()
    };
    let mut first = payload.open_stream().expect("stream");
    let error = first
        .read_chunk(&ui_context, 4)
        .expect_err("UI context must fail");
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let chunk = match first.read_chunk(&background(), 5).expect("chunk") {
        Completion::Completed(chunk) => chunk,
        Completion::Cancelled(reason) => panic!("unexpected cancel: {reason:?}"),
    };
    assert_eq!(chunk.as_deref(), Some(b"hello".as_slice()));
    assert_eq!(setup.service.active_stream_leases(), 1);

    let mut leases = vec![first];
    for _ in 1..MAX_ACTIVE_STREAM_LEASES {
        leases.push(payload.open_stream().expect("under cap"));
    }
    assert_eq!(
        setup.service.active_stream_leases(),
        MAX_ACTIVE_STREAM_LEASES
    );
    let error = payload.open_stream().expect_err("65th stream");
    assert_eq!(error.code, ErrorCode::Busy);
    let released = leases.pop().expect("lease");
    assert!(released.release());
    assert!(!released.release());
    assert_eq!(
        setup.service.active_stream_leases(),
        MAX_ACTIVE_STREAM_LEASES - 1
    );
    let mut replacement = payload.open_stream().expect("slot freed");
    let cancelled = CancellationToken::default();
    cancelled.cancel(CancelReason::Caller);
    let context = OperationContext {
        cancellation: cancelled,
        ..OperationContext::default()
    };
    match replacement.read_chunk(&context, 1).expect("cancel") {
        Completion::Cancelled(CancelReason::Caller) => {}
        other => panic!("unexpected cancellation: {other:?}"),
    }
    let deadline = Instant::now() - Duration::from_secs(1);
    let context = OperationContext {
        deadline: Some(deadline),
        ..OperationContext::default()
    };
    let error = replacement.read_chunk(&context, 1).expect_err("timed out");
    assert_eq!(error.code, ErrorCode::TimedOut);
    assert!(payload.revoke());
    assert!(!payload.revoke());
    let error = replacement
        .read_chunk(&background(), 1)
        .expect_err("revoked consumer");
    assert_eq!(error.code, ErrorCode::ResourceExpired);
    drop(leases);
    drop(replacement);
}

#[test]
fn file_resources_transfer_as_opaque_path_free_handles() {
    let setup = crate::setup(5);
    let files = FakeFileProvider::new();
    let resource = files.create_stream(
        "quarterly-report.txt",
        b"report-bytes",
        DurableGrantSupport::Unsupported,
    );
    setup
        .fake
        .set_external_files([resource.clone()])
        .expect("files");
    let outcome = completed(
        setup
            .service
            .read_clipboard(interaction(&setup), &[FormatDescriptor::file_resources()]),
    );
    let payload = outcome.matched().expect("files payload");
    assert_eq!(payload.delivery(), DeliveryMode::Eager);
    assert_eq!(payload.files().expect("files").len(), 1);
    assert!(payload.as_bytes().is_none());
    let error = payload
        .open_stream()
        .expect_err("files are not byte streams");
    assert_eq!(error.code, ErrorCode::Unsupported);

    let debug = format!("{payload:?}");
    assert!(debug.contains("Files"));
    assert!(!debug.contains("content://"));
    assert!(!debug.contains("file://"));
    let root = files.root().to_string_lossy().to_string();
    assert!(
        !debug.contains(&root),
        "no machine-local scratch root in Debug"
    );
    assert!(!format!("{:?}", resource).contains(&root));

    let ui_context = OperationContext {
        ui: Some(setup.ui.clone()),
        ..OperationContext::default()
    };
    let error = match payload.open_file_stream(0, &ui_context) {
        Err(error) => error,
        Ok(_) => panic!("UI context must not open a file stream"),
    };
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let mut reader = match payload
        .open_file_stream(0, &background())
        .expect("opaque reader")
    {
        Completion::Completed(reader) => reader,
        Completion::Cancelled(reason) => panic!("unexpected cancel: {reason:?}"),
    };
    let read = match reader.read_chunk(&background(), 64).expect("chunk") {
        Completion::Completed(chunk) => chunk,
        Completion::Cancelled(reason) => panic!("unexpected cancel: {reason:?}"),
    };
    assert_eq!(read.as_deref(), Some(b"report-bytes".as_slice()));
    let copy = resource.clone();
    drop(reader);
    drop(outcome);
    assert!(
        !copy.is_released(),
        "consumer ownership keeps the handle alive"
    );
}

#[test]
fn drag_and_drop_state_machine_evaluator_stale_callbacks_and_terminal_results() {
    let setup = crate::setup(6);
    let surface_id = surface(&setup);
    let service = setup.service.clone();

    let low = completed_target(
        service.register_drop_target(
            DropTargetSpec::new(surface_id, 1, rect(), [FormatDescriptor::plain_text()])
                .expect("low target")
                .with_z_index(1),
        ),
    );
    let high_reject_calls = Rc::new(Cell::new(0u32));
    let captured = high_reject_calls.clone();
    let high = completed_target(
        service.register_drop_target(
            DropTargetSpec::new(surface_id, 1, rect(), [FormatDescriptor::plain_text()])
                .expect("high target")
                .with_z_index(10)
                .with_evaluator(Rc::new(move |_| {
                    captured.set(captured.get().saturating_add(1));
                    DropTargetVerdict::Reject
                })),
        ),
    );
    assert_eq!(service.drop_target_ids(), vec![low.id(), high.id()]);

    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_eager(eager_text("dragged"))
        .expect("source");
    let session = completed(service.start_drag(interaction(&setup), source));
    let token = session.native_session_token().expect("native token");
    let epoch = session.session_epoch().expect("epoch");
    assert!(session.is_active());
    assert_eq!(setup.fake.active_drag_tokens(), vec![token]);

    let update =
        completed(session.drag_enter(surface_id, 1, point(10, 10), DragModifiers::default()));
    assert!(update.is_accepted(), "{update:?}");
    assert_eq!(update.target_id, Some(low.id()));
    assert_eq!(high_reject_calls.get(), 1);
    let update =
        completed(session.drag_over(surface_id, 1, point(20, 20), DragModifiers::default()));
    assert_eq!(update.target_id, Some(low.id()));
    assert_eq!(high_reject_calls.get(), 2);
    let update = completed(session.drag_leave());
    assert_eq!(update.target_id, None);

    let stale = completed(service.drag_enter(
        session.id(),
        token.saturating_add(1),
        epoch,
        surface_id,
        1,
        point(1, 1),
        DragModifiers::default(),
    ));
    assert_eq!(stale.stale, Some(StaleCallbackReason::TokenMismatch));
    let stale = completed(service.drag_over(
        session.id(),
        token,
        epoch.saturating_add(1),
        surface_id,
        1,
        point(1, 1),
        DragModifiers::default(),
    ));
    assert_eq!(stale.stale, Some(StaleCallbackReason::EpochMismatch));
    let stale = completed(service.drag_enter(
        DragSessionId(9_999),
        token,
        epoch,
        surface_id,
        1,
        point(1, 1),
        DragModifiers::default(),
    ));
    assert_eq!(stale.stale, Some(StaleCallbackReason::UnknownSession));
    let stale = completed(session.drag_over(surface_id, 2, point(1, 1), DragModifiers::default()));
    assert_eq!(stale.stale, Some(StaleCallbackReason::SurfaceNotLive));
    assert!(service.stale_callback_count() >= 4);

    let completion =
        completed(session.complete_drop(surface_id, 1, point(10, 10), DragModifiers::default()));
    let (outcome, payload) = match completion {
        DropCompletion::Dropped { outcome, payload } => (outcome, payload),
        other => panic!("unexpected drop result: {other:?}"),
    };
    assert_eq!(outcome.target_id, low.id());
    assert_eq!(outcome.format, FormatDescriptor::plain_text());
    assert_eq!(outcome.effect, DragEffect::Copy);
    assert_eq!(payload.as_text(), Some("dragged"));
    assert_eq!(setup.fake.complete_drag_calls().len(), 1);
    assert!(!session.is_active());
    assert_eq!(
        session.status(),
        Some(DragSessionStatus::Completed(outcome.clone()))
    );

    let duplicate = completed(service.complete_drop(
        session.id(),
        token,
        epoch,
        surface_id,
        1,
        point(10, 10),
        DragModifiers::default(),
    ));
    assert!(matches!(
        duplicate,
        DropCompletion::Ignored(StaleCallbackReason::SessionCompleted)
    ));
    let duplicate =
        completed(service.cancel_drag(session.id(), token, epoch, DragCancelReason::User));
    assert_eq!(duplicate, DragCancelOutcome::AlreadyTerminal);

    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_eager(eager_text("second"))
        .expect("source");
    let second = completed(service.start_drag(interaction(&setup), source));
    let second_token = second.native_session_token().expect("token");
    let second_epoch = second.session_epoch().expect("epoch");
    let cancelled = completed(second.cancel(DragCancelReason::PointerCancel));
    assert_eq!(
        cancelled,
        DragCancelOutcome::Cancelled(DragCancelReason::PointerCancel)
    );
    assert_eq!(
        setup.fake.cancel_drag_calls(),
        vec![second_token],
        "provider is notified exactly once"
    );
    let duplicate = completed(service.cancel_drag(
        second.id(),
        second_token,
        second_epoch,
        DragCancelReason::User,
    ));
    assert_eq!(duplicate, DragCancelOutcome::AlreadyTerminal);
    let stale = completed(service.drag_leave(second.id(), second_token, second_epoch));
    assert_eq!(stale.stale, Some(StaleCallbackReason::SessionCancelled));
}

fn completed_target(result: Result<DropTargetLease, SystemError>) -> DropTargetLease {
    result.expect("drop target")
}

#[test]
fn drop_evaluator_runs_reentrantly_and_effect_override_is_bounded() {
    let setup = crate::setup(7);
    let surface_id = surface(&setup);
    let service = setup.service.clone();
    let correlation = Rc::new(RefCell::new(None::<(DragSessionId, u64, u64)>));
    let captured = correlation.clone();
    let reentrant = service.clone();
    let _target = setup
        .service
        .register_drop_target(
            DropTargetSpec::new(surface_id, 1, rect(), [FormatDescriptor::plain_text()])
                .expect("target")
                .with_z_index(5)
                .with_evaluator(Rc::new(move |_| {
                    if let Some((id, token, epoch)) = *captured.borrow() {
                        let _ = reentrant.cancel_drag(id, token, epoch, DragCancelReason::User);
                    }
                    DropTargetVerdict::Accept
                })),
        )
        .expect("register");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_eager(eager_text("reentrant"))
        .expect("source");
    let session = completed(service.start_drag(interaction(&setup), source));
    *correlation.borrow_mut() = Some((
        session.id(),
        session.native_session_token().expect("token"),
        session.session_epoch().expect("epoch"),
    ));
    let update =
        completed(session.drag_over(surface_id, 1, point(10, 10), DragModifiers::default()));
    assert_eq!(update.stale, Some(StaleCallbackReason::SessionCancelled));
    assert_eq!(
        session.status(),
        Some(DragSessionStatus::Cancelled(DragCancelReason::User))
    );
    assert!(!session.is_active());

    let setup8 = crate::setup(8);
    let surface_id = surface(&setup8);
    let service = setup8.service.clone();
    let _target = service
        .register_drop_target(
            DropTargetSpec::new(surface_id, 1, rect(), [FormatDescriptor::plain_text()])
                .expect("target")
                .with_effects(
                    AllowedEffects::new([DragEffect::Copy, DragEffect::Link]).expect("effects"),
                )
                .expect("effects")
                .with_evaluator(Rc::new(|_| {
                    DropTargetVerdict::AcceptWithEffect(DragEffect::Link)
                })),
        )
        .expect("register");
    let source = DragSourceSpec::new(
        preview(),
        AllowedEffects::new([DragEffect::Copy, DragEffect::Link]).expect("source effects"),
    )
    .with_eager(eager_text("linked"))
    .expect("source");
    let session = completed(service.start_drag(interaction(&setup8), source));
    let update =
        completed(session.drag_over(surface_id, 1, point(10, 10), DragModifiers::default()));
    assert_eq!(update.effect, Some(DragEffect::Link));
    let completion =
        completed(session.complete_drop(surface_id, 1, point(10, 10), DragModifiers::default()));
    let outcome = completion.outcome().expect("drop outcome");
    assert_eq!(outcome.effect, DragEffect::Link);

    // An evaluator can never widen the allowed intersection.
    let setup9 = crate::setup(9);
    let surface_id = surface(&setup9);
    let service = setup9.service.clone();
    let _target = service
        .register_drop_target(
            DropTargetSpec::new(surface_id, 1, rect(), [FormatDescriptor::plain_text()])
                .expect("target")
                .with_effects(AllowedEffects::copy())
                .expect("effects")
                .with_evaluator(Rc::new(|_| {
                    DropTargetVerdict::AcceptWithEffect(DragEffect::Link)
                })),
        )
        .expect("register");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_eager(eager_text("bounded"))
        .expect("source");
    let session = completed(service.start_drag(interaction(&setup9), source));
    let update =
        completed(session.drag_over(surface_id, 1, point(10, 10), DragModifiers::default()));
    assert!(!update.is_accepted(), "{update:?}");
    assert_eq!(update.effect, None);
}

#[test]
fn forbidden_drop_and_delayed_drop_toctou_release_promised_resources() {
    let setup = crate::setup(10);
    let surface_id = surface(&setup);
    let promised = FakeFileProvider::new().create_stream(
        "promised-drop.txt",
        b"drop",
        DurableGrantSupport::Unsupported,
    );
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), |_| {
        Ok(Completion::Completed(TypedPayload::files([])?))
    })
    .with_promised_files([promised.clone()])
    .expect("promised");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_delayed(delayed)
        .expect("source");
    let session = completed(setup.service.start_drag(interaction(&setup), source));
    let completion =
        completed(session.complete_drop(surface_id, 1, point(10, 10), DragModifiers::default()));
    assert!(matches!(
        completion,
        DropCompletion::Cancelled(DragCancelReason::ForbiddenTarget)
    ));
    assert!(promised.is_released());
    assert_eq!(setup.fake.cancel_drag_calls().len(), 1);
    assert_eq!(
        session.status(),
        Some(DragSessionStatus::Cancelled(
            DragCancelReason::ForbiddenTarget
        ))
    );

    let setup11 = crate::setup(11);
    let surface_id = surface(&setup11);
    let service = setup11.service.clone();
    let owner = setup11.owner.clone();
    let _target = service
        .register_drop_target(
            DropTargetSpec::new(surface_id, 1, rect(), [FormatDescriptor::plain_text()])
                .expect("target"),
        )
        .expect("register");
    let callback_service = service.clone();
    let delayed = LazyRepresentation::new(FormatDescriptor::plain_text(), move |_| {
        callback_service.dispose_surface_generation(surface_id, 1);
        assert!(
            !callback_service.is_surface_live(surface_id, 1),
            "the callback must observe the disposed generation"
        );
        assert!(
            callback_service.drag_status(DragSessionId(1)).is_some(),
            "the session record remains inspectable"
        );
        Ok(Completion::Completed(
            TypedPayload::plain_text("late").expect("text"),
        ))
    });
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_delayed(delayed)
        .expect("source");
    let session = completed(service.start_drag(interaction(&setup11), source));
    let error = session
        .complete_drop(surface_id, 1, point(10, 10), DragModifiers::default())
        .expect_err("surface revoked during generation");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert!(!session.is_active());
    assert!(owner.context(None).is_ok(), "owner is host-owned here");
}

#[test]
fn lifecycle_background_and_termination_revoke_transfers() {
    let setup = crate::setup(12);
    let surface_id = surface(&setup);
    let calls = Rc::new(Cell::new(0u32));
    let captured = calls.clone();
    let delayed = LazyRepresentation::new(FormatDescriptor::html(), move |_| {
        captured.set(captured.get().saturating_add(1));
        Ok(Completion::Completed(
            TypedPayload::html("<i>late</i>").expect("html"),
        ))
    });
    let write = RichClipboardWrite::application()
        .with_eager(eager_text("eager survives"))
        .expect("eager")
        .with_delayed(delayed)
        .expect("write");
    let _lease = completed(setup.service.write_clipboard(interaction(&setup), write));

    let background = LifecycleState::from_parts(
        AppPhase::Background,
        [],
        gpui_box::lifecycle::SessionLockState::Unlocked,
        gpui_box::lifecycle::MemoryPressure::Normal,
    );
    setup.service.notify_lifecycle(&background);
    let error = setup
        .service
        .user_initiated_with(setup.ui.clone())
        .expect_err("background gesture");
    assert_eq!(error.code, ErrorCode::PermissionDenied);

    let terminating = LifecycleState::from_parts(
        AppPhase::Terminating,
        [],
        gpui_box::lifecycle::SessionLockState::Unlocked,
        gpui_box::lifecycle::MemoryPressure::Normal,
    );
    setup.service.notify_lifecycle(&terminating);
    assert_eq!(setup.fake.released_delayed_tokens().len(), 1);
    let error = setup
        .service
        .user_initiated_for_surface(surface_id, 1)
        .expect_err("terminating");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert_eq!(calls.get(), 0);
    assert!(
        setup
            .fake
            .clipboard_entries()
            .iter()
            .any(|entry| matches!(entry, ClipboardEntry::Eager(_))),
        "eager clipboard bytes survive termination"
    );

    let setup13 = crate::setup(13);
    let delayed = LazyRepresentation::new(FormatDescriptor::html(), |_| {
        Ok(Completion::Completed(
            TypedPayload::html("<i>x</i>").expect("html"),
        ))
    });
    let write = RichClipboardWrite::application()
        .with_delayed(delayed)
        .expect("write");
    let _lease = completed(
        setup13
            .service
            .write_clipboard(interaction(&setup13), write),
    );
    setup13.service.notify_source_exit();
    assert_eq!(setup13.fake.released_delayed_tokens().len(), 1);
}

#[test]
fn web_delayed_representations_are_rejected_while_eager_text_works() {
    let fake = FakeRichTransferProvider::with_platform(PlatformTarget::Web);
    let service = RichTransferService::from_fake(fake.clone());
    let owner = UiOwner::new(SurfaceId(14));
    let ui = owner.context(None).expect("ui");
    service
        .attach_surface(SurfaceId(14), 1, true)
        .expect("surface");

    let delayed = LazyRepresentation::new(FormatDescriptor::html(), |_| {
        Ok(Completion::Completed(
            TypedPayload::html("<b>x</b>").expect("html"),
        ))
    });
    let write = RichClipboardWrite::application()
        .with_delayed(delayed)
        .expect("write");
    let interaction = service.user_initiated_with(ui.clone()).expect("gesture");
    let error = service
        .write_clipboard(interaction, write)
        .expect_err("web delayed clipboard");
    assert_eq!(error.code, ErrorCode::Unsupported);

    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_delayed(LazyRepresentation::new(
            FormatDescriptor::plain_text(),
            |_| {
                Ok(Completion::Completed(
                    TypedPayload::plain_text("late").expect("text"),
                ))
            },
        ))
        .expect("source");
    let interaction = service.user_initiated_with(ui.clone()).expect("gesture");
    let error = service
        .start_drag(interaction, source)
        .expect_err("web delayed drag");
    assert_eq!(error.code, ErrorCode::Unsupported);

    let write = RichClipboardWrite::application()
        .with_eager(eager_text("web eager"))
        .expect("eager");
    let interaction = service.user_initiated_with(ui.clone()).expect("gesture");
    let _lease = completed(service.write_clipboard(interaction, write));
    let interaction = service.user_initiated_with(ui).expect("gesture");
    let outcome = completed(service.read_clipboard(interaction, &[FormatDescriptor::plain_text()]));
    assert_eq!(
        outcome.matched().and_then(ConsumedPayload::as_text),
        Some("web eager")
    );
    assert_eq!(fake.publish_call_count(), 1);
}

#[test]
fn bounded_event_stream_reports_resync_without_blocking_producers() {
    let fake = FakeRichTransferProvider::new();
    let service = RichTransferService::with_event_capacity(Rc::new(fake.clone()), 1);
    let owner = UiOwner::new(SurfaceId(15));
    let ui = owner.context(None).expect("ui");
    service
        .attach_surface(SurfaceId(15), 1, true)
        .expect("surface");
    let subscription = service.subscribe();
    let first = completed(
        service.write_clipboard(
            service.user_initiated_with(ui.clone()).expect("gesture"),
            RichClipboardWrite::application()
                .with_eager(eager_text("one"))
                .expect("write"),
        ),
    );
    let _second = completed(
        service.write_clipboard(
            service.user_initiated_with(ui.clone()).expect("gesture"),
            RichClipboardWrite::application()
                .with_eager(eager_text("two"))
                .expect("write"),
        ),
    );
    assert_eq!(next_payload(&subscription), Some(Event::ResyncRequired));
    drop(first);
}

#[test]
fn events_cancel_and_surface_cleanup_are_public_safe() {
    let setup = crate::setup(16);
    let surface_id = surface(&setup);
    let subscription = setup.service.subscribe();
    let _target = setup
        .service
        .register_drop_target(
            DropTargetSpec::new(surface_id, 1, rect(), [FormatDescriptor::plain_text()])
                .expect("target"),
        )
        .expect("register");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_eager(eager_text("event drag"))
        .expect("source");
    let session = completed(setup.service.start_drag(interaction(&setup), source));
    let token = session.native_session_token().expect("token");
    let epoch = session.session_epoch().expect("epoch");
    let _ = completed(setup.service.drag_over(
        session.id(),
        token,
        epoch,
        surface_id,
        1,
        point(10, 10),
        DragModifiers::default(),
    ));
    setup.service.dispose_surface_generation(surface_id, 1);
    assert!(!session.is_active());
    assert_eq!(
        session.status(),
        Some(DragSessionStatus::Cancelled(
            DragCancelReason::SourceSurfaceDisposed
        ))
    );
    let mut events = Vec::new();
    while let Some(payload) = next_payload(&subscription) {
        events.push(payload);
    }
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(DndEvent::DragCancelled {
            reason: DragCancelReason::SourceSurfaceDisposed,
            ..
        })
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Value(DndEvent::SurfaceDisposed { .. })))
    );
    for event in &events {
        let debug = format!("{event:?}");
        assert!(!debug.contains("content://"));
        assert!(!debug.contains("file://"));
    }
}

#[test]
fn unsupported_provider_fails_closed_and_capability_snapshots_stay_static() {
    let provider = Rc::new(UnsupportedRichTransferProvider::default());
    let service = RichTransferService::new(provider);
    let owner = UiOwner::new(SurfaceId(17));
    let ui = owner.context(None).expect("ui");
    service
        .attach_surface(SurfaceId(17), 1, true)
        .expect("surface");
    assert!(
        service
            .capabilities()
            .iter()
            .all(|capability| capability.support == Support::Unsupported
                && capability.availability == Availability::Unavailable)
    );
    let error = service
        .read_clipboard(
            service.user_initiated_with(ui.clone()).expect("gesture"),
            &[FormatDescriptor::plain_text()],
        )
        .expect_err("unsupported read");
    assert_eq!(error.code, ErrorCode::Unsupported);
    let error = service
        .register_drop_target(
            DropTargetSpec::new(SurfaceId(17), 1, rect(), [FormatDescriptor::plain_text()])
                .expect("target"),
        )
        .expect_err("unsupported target");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[cfg(feature = "dnd-gpui")]
#[test]
fn dnd_gpui_feature_exposes_adapter_and_system_registration() {
    fn assert_adapter_symbols() {
        let _init = gpui_box::dnd::gpui_adapter::init_dnd;
        let _lookup = gpui_box::dnd::gpui_adapter::dnd_service;
        let _install = gpui_box::dnd::gpui_adapter::install_dnd_service;
        let _install_services = gpui_box::dnd::gpui_adapter::install_dnd_in_services;
        let _sync = gpui_box::dnd::gpui_adapter::sync_lifecycle;
        let _entity = gpui_box::dnd::gpui_adapter::DndEntityModel::new;
        let _scope = gpui_box::dnd::gpui_adapter::DndSurfaceScope::new;
        let _scope_try = gpui_box::dnd::gpui_adapter::DndSurfaceScope::try_new;
    }
    assert_adapter_symbols();

    let fake = FakeRichTransferProvider::new();
    let service = RichTransferService::from_fake(fake);
    let mut services = SystemServices::new();
    gpui_box::dnd::gpui_adapter::install_dnd_in_services(&mut services, service.clone())
        .expect("registration");
    let registered = services
        .service::<RichTransferService>()
        .expect("registered service");
    assert!(service.shares_state_with(&registered));

    let lifecycle = gpui_box::lifecycle::LifecycleState::from_parts(
        gpui_box::lifecycle::AppPhase::ForegroundActive,
        [],
        gpui_box::lifecycle::SessionLockState::Unlocked,
        gpui_box::lifecycle::MemoryPressure::Normal,
    );
    gpui_box::dnd::gpui_adapter::sync_lifecycle(&service, &lifecycle);

    let owner = UiOwner::new(SurfaceId(81));
    let old = gpui_box::dnd::gpui_adapter::DndSurfaceScope::try_new(owner.clone(), service.clone())
        .expect("old scope");
    assert_eq!(old.generation(), 1);
    let old_target = old
        .register_drop_target_at(rect(), [FormatDescriptor::plain_text()])
        .expect("old target");
    owner.advance_generation().expect("advance");
    let new = gpui_box::dnd::gpui_adapter::DndSurfaceScope::new(owner.clone(), service.clone());
    assert_eq!(new.generation(), 2);
    let new_target = new
        .register_drop_target_at(rect(), [FormatDescriptor::plain_text()])
        .expect("new target");
    drop(old);
    assert_eq!(service.drop_target_ids(), vec![new_target.id()]);
    assert!(!old_target.is_active());
    assert!(owner.context(None).is_ok(), "new generation survives");
    drop(new);
    assert!(owner.context(None).is_err());
    assert!(
        gpui_box::dnd::gpui_adapter::DndSurfaceScope::try_new(owner, service).is_err(),
        "disposed owner cannot create a scope"
    );
}

#[test]
fn mid_drag_start_toctou_cancels_provider_and_releases_promised_resources() {
    let setup = crate::setup(18);
    let surface_id = surface(&setup);
    let promised = FakeFileProvider::new().create_stream(
        "mid-drag.txt",
        b"drag",
        DurableGrantSupport::Unsupported,
    );
    let hook_service = setup.service.clone();
    let hook_promised = promised.clone();
    setup.fake.set_on_start_drag(Some(Rc::new(move |_| {
        hook_service.dispose_surface_generation(SurfaceId(18), 1);
        assert!(hook_promised.is_released());
    })));
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), |_| {
        Ok(Completion::Completed(TypedPayload::files([])?))
    })
    .with_promised_files([promised.clone()])
    .expect("promised");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_delayed(delayed)
        .expect("source");
    let error = setup
        .service
        .start_drag(interaction(&setup), source)
        .expect_err("mid-start disposal");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert!(promised.is_released());
    assert_eq!(setup.fake.start_drag_call_count(), 1);
    assert_eq!(setup.fake.cancel_drag_calls().len(), 1);
    assert!(setup.fake.active_drag_tokens().is_empty());
    assert!(setup.service.drop_target_ids().is_empty());
    let _ = surface_id;
}

#[test]
fn drag_session_lease_drop_cancels_and_source_exit_revokes_everything() {
    let setup = crate::setup(19);
    let promised = FakeFileProvider::new().create_stream(
        "lease-drop.txt",
        b"drop",
        DurableGrantSupport::Unsupported,
    );
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), |_| {
        Ok(Completion::Completed(TypedPayload::files([])?))
    })
    .with_promised_files([promised.clone()])
    .expect("promised");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_delayed(delayed)
        .expect("source");
    let session = completed(setup.service.start_drag(interaction(&setup), source));
    let id = session.id();
    drop(session);
    assert_eq!(
        setup.service.drag_status(id),
        Some(DragSessionStatus::Cancelled(DragCancelReason::LeaseDropped))
    );
    assert!(promised.is_released());
    assert_eq!(setup.fake.cancel_drag_calls().len(), 1);
    assert!(setup.fake.active_drag_tokens().is_empty());

    let setup = crate::setup(20);
    let promised = FakeFileProvider::new().create_stream(
        "source-exit.txt",
        b"exit",
        DurableGrantSupport::Unsupported,
    );
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), |_| {
        Ok(Completion::Completed(TypedPayload::files([])?))
    })
    .with_promised_files([promised.clone()])
    .expect("promised");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_delayed(delayed)
        .expect("source");
    let session = completed(setup.service.start_drag(interaction(&setup), source));
    setup.service.notify_source_exit();
    assert!(promised.is_released());
    assert_eq!(
        session.status(),
        Some(DragSessionStatus::Cancelled(DragCancelReason::SourceExited))
    );
    assert_eq!(setup.fake.cancel_drag_calls().len(), 1);
}

#[test]
fn delayed_read_revocation_during_generation_fails_cleanly() {
    let setup = crate::setup(21);
    let calls = Rc::new(Cell::new(0u32));
    let captured = calls.clone();
    let callback_service = setup.service.clone();
    let delayed = LazyRepresentation::new(FormatDescriptor::html(), move |_| {
        captured.set(captured.get().saturating_add(1));
        callback_service.notify_source_exit();
        Ok(Completion::Completed(
            TypedPayload::html("<b>late</b>").expect("html"),
        ))
    });
    let write = RichClipboardWrite::application()
        .with_delayed(delayed)
        .expect("write");
    let _lease = completed(setup.service.write_clipboard(interaction(&setup), write));
    let error = setup
        .service
        .read_clipboard(interaction(&setup), &[FormatDescriptor::html()])
        .expect_err("handoff revoked during generation");
    assert_eq!(error.code, ErrorCode::ResourceExpired);
    assert_eq!(calls.get(), 1);
    assert_eq!(setup.fake.released_delayed_tokens().len(), 1);
}

#[test]
fn target_surface_disposal_uses_target_reason_and_external_drag_is_observable() {
    let setup = crate::setup(22);
    let target_owner = UiOwner::new(SurfaceId(23));
    let target_ui = target_owner.context(None).expect("target ui");
    setup
        .service
        .attach_surface(SurfaceId(23), 1, true)
        .expect("target surface");
    let target = setup
        .service
        .register_drop_target(
            DropTargetSpec::new(SurfaceId(23), 1, rect(), [FormatDescriptor::plain_text()])
                .expect("target"),
        )
        .expect("register");
    let source = DragSourceSpec::new(preview(), AllowedEffects::all())
        .with_eager(eager_text("cross surface"))
        .expect("source");
    let session = completed(setup.service.start_drag(interaction(&setup), source));
    let update =
        completed(session.drag_over(SurfaceId(23), 1, point(10, 10), DragModifiers::default()));
    assert_eq!(update.target_id, Some(target.id()));
    setup.service.dispose_surface_generation(SurfaceId(23), 1);
    assert_eq!(
        session.status(),
        Some(DragSessionStatus::Cancelled(
            DragCancelReason::TargetSurfaceDisposed
        ))
    );
    assert_eq!(setup.fake.cancel_drag_calls().len(), 1);
    assert!(!target.is_active());
    let _ = target_ui;

    let subscription = setup.service.subscribe();
    let token = setup.fake.simulate_external_drag_enter(
        surface(&setup),
        1,
        point(5, 5),
        [PayloadOffer::new(
            FormatDescriptor::plain_text(),
            DeliveryMode::Eager,
        )],
    );
    assert_eq!(setup.service.external_drag_notice_count(), 1);
    let event = next_payload(&subscription).expect("external drag event");
    match event {
        Event::Value(DndEvent::ExternalDragEntered {
            native_session_token,
            surface: event_surface,
            formats,
            ..
        }) => {
            assert_eq!(native_session_token, token);
            assert_eq!(event_surface, setup.ui.surface);
            assert_eq!(formats, vec![FormatDescriptor::plain_text()]);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    setup.fake.simulate_external_drag_leave(token);
    match next_payload(&subscription).expect("external drag leave event") {
        Event::Value(DndEvent::ExternalDragLeft {
            native_session_token,
        }) => assert_eq!(native_session_token, token),
        other => panic!("unexpected event: {other:?}"),
    }

    let drop_token = setup.fake.simulate_external_drag_enter(
        surface(&setup),
        1,
        point(6, 6),
        [PayloadOffer::new(
            FormatDescriptor::plain_text(),
            DeliveryMode::Eager,
        )],
    );
    match next_payload(&subscription).expect("second external drag event") {
        Event::Value(DndEvent::ExternalDragEntered {
            native_session_token,
            ..
        }) => assert_eq!(native_session_token, drop_token),
        other => panic!("unexpected event: {other:?}"),
    }
    setup.fake.simulate_external_drop(drop_token, point(7, 7));
    match next_payload(&subscription).expect("external drop event") {
        Event::Value(DndEvent::ExternalDrop {
            native_session_token,
            ..
        }) => assert_eq!(native_session_token, drop_token),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn stale_external_drag_notices_are_ignored() {
    let setup = crate::setup(24);
    let surface = surface(&setup);
    let offer = || PayloadOffer::new(FormatDescriptor::plain_text(), DeliveryMode::Eager);

    let subscription = setup.service.subscribe();
    setup
        .fake
        .simulate_external_drag_enter_with_token(0, surface, 1, point(1, 1), [offer()]);
    assert_eq!(setup.service.external_drag_notice_count(), 0);
    assert_eq!(
        setup.service.last_stale_callback(),
        Some(StaleCallbackReason::TokenMismatch)
    );
    match next_payload(&subscription).expect("zero-token stale event") {
        Event::Value(DndEvent::StaleCallbackIgnored {
            session: None,
            reason: StaleCallbackReason::TokenMismatch,
        }) => {}
        other => panic!("unexpected event: {other:?}"),
    }

    setup.service.dispose_surface_generation(surface, 1);
    let subscription = setup.service.subscribe();
    setup
        .fake
        .simulate_external_drag_enter_with_token(7, surface, 1, point(2, 2), [offer()]);
    assert_eq!(setup.service.external_drag_notice_count(), 0);
    assert_eq!(
        setup.service.last_stale_callback(),
        Some(StaleCallbackReason::SurfaceNotLive)
    );
    match next_payload(&subscription).expect("stale-generation event") {
        Event::Value(DndEvent::StaleCallbackIgnored {
            session: None,
            reason: StaleCallbackReason::SurfaceNotLive,
        }) => {}
        other => panic!("unexpected event: {other:?}"),
    }

    setup
        .service
        .attach_surface(surface, 2, true)
        .expect("new surface");
    let subscription = setup.service.subscribe();
    let token =
        setup
            .fake
            .simulate_external_drag_enter_with_token(11, surface, 2, point(3, 3), [offer()]);
    assert_eq!(setup.service.external_drag_notice_count(), 1);
    match next_payload(&subscription).expect("valid external drag") {
        Event::Value(DndEvent::ExternalDragEntered {
            native_session_token,
            ..
        }) => assert_eq!(native_session_token, token),
        other => panic!("unexpected event: {other:?}"),
    }
    setup.fake.simulate_external_drag_leave(token);
    match next_payload(&subscription).expect("valid leave") {
        Event::Value(DndEvent::ExternalDragLeft {
            native_session_token,
        }) => assert_eq!(native_session_token, token),
        other => panic!("unexpected event: {other:?}"),
    }
    setup.fake.simulate_external_drag_leave(token);
    assert_eq!(
        setup.service.last_stale_callback(),
        Some(StaleCallbackReason::UnknownSession)
    );
    match next_payload(&subscription).expect("duplicate leave stale") {
        Event::Value(DndEvent::StaleCallbackIgnored {
            session: None,
            reason: StaleCallbackReason::UnknownSession,
        }) => {}
        other => panic!("unexpected event: {other:?}"),
    }
    setup.fake.simulate_external_drop(token, point(4, 4));
    match next_payload(&subscription).expect("drop after leave stale") {
        Event::Value(DndEvent::StaleCallbackIgnored {
            session: None,
            reason: StaleCallbackReason::UnknownSession,
        }) => {}
        other => panic!("unexpected event: {other:?}"),
    }

    setup.fake.simulate_external_drag_leave(0);
    assert_eq!(
        setup.service.last_stale_callback(),
        Some(StaleCallbackReason::TokenMismatch)
    );
    setup.fake.simulate_external_drop(0, point(5, 5));
    assert_eq!(
        setup.service.last_stale_callback(),
        Some(StaleCallbackReason::TokenMismatch)
    );
    let _ = next_payload(&subscription);
    let _ = next_payload(&subscription);

    let token = setup
        .fake
        .simulate_external_drag_enter(surface, 2, point(6, 6), [offer()]);
    assert_eq!(setup.service.external_drag_notice_count(), 2);
    let _ = next_payload(&subscription);
    setup.fake.simulate_external_drop(token, point(7, 7));
    match next_payload(&subscription).expect("valid drop") {
        Event::Value(DndEvent::ExternalDrop {
            native_session_token,
            ..
        }) => assert_eq!(native_session_token, token),
        other => panic!("unexpected event: {other:?}"),
    }

    let token = setup
        .fake
        .simulate_external_drag_enter(surface, 2, point(8, 8), [offer()]);
    assert_eq!(setup.service.external_drag_notice_count(), 3);
    let _ = next_payload(&subscription);
    setup.fake.simulate_source_exit();
    setup.fake.simulate_external_drag_leave(token);
    assert_eq!(
        setup.service.last_stale_callback(),
        Some(StaleCallbackReason::UnknownSession)
    );
    match next_payload(&subscription).expect("source-exit stale") {
        Event::Value(DndEvent::StaleCallbackIgnored {
            session: None,
            reason: StaleCallbackReason::UnknownSession,
        }) => {}
        other => panic!("unexpected event: {other:?}"),
    }

    setup
        .service
        .attach_surface(surface, 3, true)
        .expect("third surface");
    let token = setup
        .fake
        .simulate_external_drag_enter(surface, 3, point(9, 9), [offer()]);
    assert_eq!(setup.service.external_drag_notice_count(), 4);
    let _ = next_payload(&subscription);
    setup.service.dispose_surface_generation(surface, 3);
    let subscription = setup.service.subscribe();
    setup.fake.simulate_external_drag_leave(token);
    assert_eq!(
        setup.service.last_stale_callback(),
        Some(StaleCallbackReason::UnknownSession)
    );
    match next_payload(&subscription).expect("surface-purge stale") {
        Event::Value(DndEvent::StaleCallbackIgnored {
            session: None,
            reason: StaleCallbackReason::UnknownSession,
        }) => {}
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn mid_publish_cancellation_preserves_previous_handoff_and_no_dead_record() {
    let setup = crate::setup(25);
    let existing = completed(
        setup.service.write_clipboard(
            interaction(&setup),
            RichClipboardWrite::application()
                .with_eager(eager_text("old"))
                .expect("write"),
        ),
    );
    let existing_id = existing.id();
    let promised = FakeFileProvider::new().create_stream(
        "cancel.txt",
        b"cancel",
        DurableGrantSupport::Unsupported,
    );
    let cancellation = CancellationToken::default();
    let hook_cancellation = cancellation.clone();
    let published = Rc::new(Cell::new(None::<(HandoffId, u64)>));
    let hook_published = Rc::clone(&published);
    setup.fake.set_on_publish(Some(Rc::new(move |event| {
        hook_published.set(Some((event.handoff_id, event.native_handoff_token)));
        hook_cancellation.cancel(CancelReason::User);
    })));
    let delayed = LazyRepresentation::new(FormatDescriptor::file_resources(), |_| {
        Ok(Completion::Completed(TypedPayload::files([])?))
    })
    .with_promised_files([promised.clone()])
    .expect("promised");
    let write = RichClipboardWrite::application()
        .with_eager(eager_text("cancelled"))
        .expect("eager")
        .with_delayed(delayed)
        .expect("delayed");
    let result = setup
        .service
        .write_clipboard(interaction(&setup).with_cancellation(cancellation), write)
        .expect("cancelled write");
    assert!(matches!(result, Completion::Cancelled(CancelReason::User)));
    let (cancelled_id, cancelled_token) = published.get().expect("published id");
    assert_ne!(cancelled_id, existing_id);
    assert!(existing.is_active());
    assert!(promised.is_released());
    assert_eq!(
        setup
            .fake
            .released_delayed_tokens()
            .into_iter()
            .filter(|token| *token == cancelled_token)
            .count(),
        1
    );
    setup.fake.set_on_publish(None);

    let subscription = setup.service.subscribe();
    let replacement = completed(
        setup.service.write_clipboard(
            interaction(&setup),
            RichClipboardWrite::application()
                .with_eager(eager_text("replacement"))
                .expect("write"),
        ),
    );
    assert!(replacement.is_active());
    assert!(!existing.is_active());

    let mut revoked = Vec::new();
    while let Some(event) = next_payload(&subscription) {
        if let Event::Value(DndEvent::HandoffRevoked { handoff_id, .. }) = event {
            revoked.push(handoff_id);
        }
    }
    assert_eq!(revoked, vec![existing_id]);
    assert_eq!(
        setup
            .fake
            .released_delayed_tokens()
            .into_iter()
            .filter(|token| *token == cancelled_token)
            .count(),
        1
    );
}
