use super::*;
use gpui_system::{CancelReason, SurfaceId, UiOwner};
use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

#[derive(Default)]
struct Fake {
    value: RefCell<Option<String>>,
    calls: Cell<usize>,
    failure: Cell<Option<ErrorCode>>,
    cancel_on_write: RefCell<Option<CancellationToken>>,
}
impl ClipboardProvider for Fake {
    fn capabilities(&self) -> Vec<Capability<ClipboardOperation>> {
        capabilities(
            Availability::Available,
            Support::Native,
            Evidence::ContractOnly,
        )
    }
    fn read_text(&self, _: &OperationContext) -> ServiceResult<ClipboardText> {
        self.calls.set(self.calls.get() + 1);
        if let Some(code) = self.failure.get() {
            return Err(error(code, "Fake clipboard failure"));
        }
        Ok(Completion::Completed(
            self.value
                .borrow()
                .as_ref()
                .map_or(ClipboardText::Empty, |text| {
                    ClipboardText::Text(text.clone())
                }),
        ))
    }
    fn write_text(&self, text: &str, _: &OperationContext) -> ServiceResult<ClipboardReceipt> {
        self.calls.set(self.calls.get() + 1);
        if let Some(code) = self.failure.get() {
            return Err(error(code, "Fake clipboard failure"));
        }
        *self.value.borrow_mut() = Some(text.to_owned());
        if let Some(token) = self.cancel_on_write.borrow().as_ref() {
            token.cancel(CancelReason::Caller);
        }
        Ok(Completion::Completed(ClipboardReceipt {
            sequence: Some(self.calls.get() as u32),
        }))
    }
}
fn fixture() -> (UiOwner, Rc<Fake>, Clipboard) {
    let owner = UiOwner::new(SurfaceId(7));
    let fake = Rc::new(Fake::default());
    let clipboard = Clipboard::new(fake.clone(), owner.context(None).unwrap()).unwrap();
    (owner, fake, clipboard)
}
#[test]
fn unicode_and_explicit_empty_text_round_trip_without_normalization() {
    let (_owner, _fake, clipboard) = fixture();
    assert_eq!(
        clipboard
            .read_text(clipboard.user_initiated().unwrap())
            .unwrap(),
        Completion::Completed(ClipboardText::Empty)
    );
    for text in ["中文剪贴板 🧑🏽‍💻\r\n日本語 e\u{301}", ""] {
        assert!(matches!(
            clipboard.write_text(text, clipboard.user_initiated().unwrap()),
            Ok(Completion::Completed(_))
        ));
        assert_eq!(
            clipboard
                .read_text(clipboard.user_initiated().unwrap())
                .unwrap(),
            Completion::Completed(ClipboardText::Text(text.to_owned()))
        );
    }
}
#[test]
fn cancelled_and_elapsed_interactions_never_call_provider() {
    let (_owner, fake, clipboard) = fixture();
    let cancellation = CancellationToken::default();
    cancellation.cancel(CancelReason::Caller);
    let interaction = clipboard
        .user_initiated()
        .unwrap()
        .with_cancellation(cancellation);
    assert_eq!(
        clipboard.write_text("text", interaction).unwrap(),
        Completion::Cancelled(CancelReason::Caller)
    );
    let interaction = clipboard
        .user_initiated()
        .unwrap()
        .with_deadline(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        clipboard.read_text(interaction).unwrap_err().code,
        ErrorCode::TimedOut
    );
    assert_eq!(fake.calls.get(), 0);
}
#[test]
fn consumed_gesture_is_bound_to_service_and_attachment() {
    let (owner, fake, clipboard) = fixture();
    let second = Clipboard::new(fake.clone(), owner.context(None).unwrap()).unwrap();
    let interaction = clipboard.user_initiated().unwrap();
    assert_eq!(
        second.read_text(interaction).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    let interaction = clipboard.user_initiated().unwrap();
    owner.advance_generation().unwrap();
    assert_eq!(
        clipboard.read_text(interaction).unwrap_err().code,
        ErrorCode::ContextExpired
    );
    assert!(clipboard.user_initiated().is_err());
    let current = Clipboard::new(fake.clone(), owner.context(None).unwrap()).unwrap();
    let interaction = current.user_initiated().unwrap();
    owner.dispose();
    assert_eq!(
        current.write_text("text", interaction).unwrap_err().code,
        ErrorCode::ContextExpired
    );
    assert_eq!(fake.calls.get(), 0);
}
#[test]
fn dropped_owner_invalidates_outstanding_interaction() {
    let (owner, fake, clipboard) = fixture();
    let interaction = clipboard.user_initiated().unwrap();
    drop(owner);
    assert_eq!(
        clipboard.read_text(interaction).unwrap_err().code,
        ErrorCode::ContextExpired
    );
    assert_eq!(fake.calls.get(), 0);
}
#[test]
fn denied_unavailable_and_busy_are_actionable_errors() {
    let (_owner, fake, clipboard) = fixture();
    for code in [
        ErrorCode::PermissionDenied,
        ErrorCode::Unavailable,
        ErrorCode::Busy,
    ] {
        fake.failure.set(Some(code));
        assert_eq!(
            clipboard
                .read_text(clipboard.user_initiated().unwrap())
                .unwrap_err()
                .code,
            code
        );
        assert_eq!(
            clipboard
                .write_text("text", clipboard.user_initiated().unwrap())
                .unwrap_err()
                .code,
            code
        );
    }
}
#[test]
fn rejected_external_text_and_writes_are_bounded() {
    let (_owner, fake, clipboard) = fixture();
    for text in ["x".repeat(MAX_TEXT_BYTES + 1), "nul\0text".to_owned()] {
        assert_eq!(
            clipboard
                .write_text(&text, clipboard.user_initiated().unwrap())
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
        assert_eq!(fake.calls.get(), 0);
        *fake.value.borrow_mut() = Some(text);
        assert_eq!(
            clipboard
                .read_text(clipboard.user_initiated().unwrap())
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
        fake.calls.set(0);
    }
}
#[test]
fn successful_side_effect_is_not_relabelled_as_cancellation() {
    let (_owner, fake, clipboard) = fixture();
    let token = CancellationToken::default();
    *fake.cancel_on_write.borrow_mut() = Some(token.clone());
    assert!(matches!(
        clipboard.write_text(
            "committed",
            clipboard.user_initiated().unwrap().with_cancellation(token)
        ),
        Ok(Completion::Completed(_))
    ));
    assert_eq!(fake.value.borrow().as_deref(), Some("committed"));
}
#[test]
fn unavailable_provider_never_claims_native_success() {
    let owner = UiOwner::new(SurfaceId(1));
    let clipboard =
        Clipboard::new(Rc::new(UnavailableClipboard), owner.context(None).unwrap()).unwrap();
    assert_eq!(clipboard.capabilities()[0].support, Support::Unsupported);
    assert_eq!(
        clipboard
            .read_text(clipboard.user_initiated().unwrap())
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
}
#[cfg(target_os = "windows")]
#[test]
fn invalid_native_attachment_fails_without_opening_clipboard() {
    let owner = UiOwner::new(SurfaceId(1));
    let failure = WindowsClipboard::attach(
        std::num::NonZeroIsize::new(-1).unwrap(),
        owner.context(None).unwrap(),
    );
    assert_eq!(failure.err().unwrap().code, ErrorCode::ContextExpired);
}
