use gpui_secrets::*;
use gpui_system::{
    Availability, Capability, Completion, Configuration, ErrorCode, Evidence, OperationContext,
    PermissionState, Support, SystemError,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Default)]
struct Fake {
    records: Mutex<BTreeMap<String, SecretBytes>>,
    mode: AtomicUsize,
    writes: AtomicUsize,
    deleted: AtomicUsize,
    cancel_policy: Mutex<Option<gpui_system::CancellationToken>>,
}
impl Fake {
    fn state<T>(&self, value: T) -> ProviderResult<T> {
        match self.mode.load(Ordering::SeqCst) {
            1 => Ok(SecretAccess::Locked),
            2 => Ok(SecretAccess::KeyInvalidated),
            3 => Err(SystemError::new(
                ErrorCode::PermissionDenied,
                "Secure access denied",
            )),
            _ => Ok(SecretAccess::Available(value)),
        }
    }
}
impl CredentialProvider for Fake {
    fn capabilities(&self) -> Vec<Capability<SecretOperation>> {
        [
            SecretOperation::Put,
            SecretOperation::Get,
            SecretOperation::Delete,
            SecretOperation::RemoveAccount,
        ]
        .into_iter()
        .map(|operation| Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: vec![],
            observed_revision: 1,
        })
        .collect()
    }
    fn validate_policy(&self, policy: SecretPolicy) -> Result<(), SystemError> {
        if let Some(token) = self.cancel_policy.lock().unwrap().as_ref() {
            token.cancel(gpui_system::CancelReason::Caller);
        }
        if policy != SecretPolicy::default() {
            Err(SystemError::new(
                ErrorCode::Unsupported,
                "Unsupported protection requirement",
            ))
        } else {
            Ok(())
        }
    }
    fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.records
            .lock()
            .unwrap()
            .insert(target.into(), SecretBytes::new(value.expose().to_vec())?);
        Ok(SecretAccess::Available(()))
    }
    fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>> {
        let value = self
            .records
            .lock()
            .unwrap()
            .get(target)
            .map(|v| SecretBytes::new(v.expose().to_vec()).unwrap());
        self.state(value)
    }
    fn delete(&self, target: &str) -> ProviderResult<DeleteReceipt> {
        if self.mode.load(Ordering::SeqCst) == 4 && self.deleted.load(Ordering::SeqCst) > 0 {
            return Err(SystemError::new(
                ErrorCode::PlatformFailure,
                "Injected deletion failure",
            ));
        }
        self.deleted.fetch_add(1, Ordering::SeqCst);
        let existed = self.records.lock().unwrap().remove(target).is_some();
        Ok(SecretAccess::Available(DeleteReceipt { existed }))
    }
    fn list(&self, prefix: &str) -> ProviderResult<Vec<String>> {
        if self.mode.load(Ordering::SeqCst) == 5 {
            return Ok(SecretAccess::Available(vec!["unrelated-credential".into()]));
        }
        self.state(
            self.records
                .lock()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect(),
        )
    }
}
fn key(account: &str) -> CredentialKey {
    CredentialKey::new(account, "service", "token").unwrap()
}
fn completed<T>(result: gpui_system::ServiceResult<T>) -> T {
    match result.unwrap() {
        Completion::Completed(value) => value,
        Completion::Cancelled(_) => panic!("unexpected cancellation"),
    }
}
fn available<T>(result: gpui_system::ServiceResult<SecretAccess<T>>) -> T {
    match completed(result) {
        SecretAccess::Available(value) => value,
        _ => panic!("provider unavailable"),
    }
}
#[test]
fn accounts_and_application_identity_are_isolated_and_cleanup_is_idempotent() {
    let provider = Arc::new(Fake::default());
    let a = Secrets::with_provider("app", provider.clone()).unwrap();
    let b = Secrets::with_provider("other-app", provider).unwrap();
    let cx = OperationContext::default();
    for (service, account, value) in [(&a, "a", 1), (&a, "a/", 2), (&b, "a", 3)] {
        available(service.put(
            &key(account),
            SecretBytes::new(vec![value]).unwrap(),
            SecretPolicy::default(),
            &cx,
        ));
    }
    assert_eq!(available(a.get(&key("a"), &cx)).unwrap().expose(), &[1]);
    let receipt = completed(a.remove_account("a", &cx));
    assert_eq!(receipt.deleted, 1);
    assert_eq!(receipt.state, CleanupState::Complete);
    assert!(available(a.get(&key("a"), &cx)).is_none());
    assert_eq!(available(a.get(&key("a/"), &cx)).unwrap().expose(), &[2]);
    assert_eq!(available(b.get(&key("a"), &cx)).unwrap().expose(), &[3]);
    assert_eq!(completed(a.remove_account("a", &cx)).deleted, 0);
}
#[test]
fn provider_states_remain_distinct_and_diagnostics_have_no_payload() {
    let provider = Arc::new(Fake::default());
    let service = Secrets::with_provider("private-app", provider.clone()).unwrap();
    let cx = OperationContext::default();
    provider.mode.store(1, Ordering::SeqCst);
    assert!(matches!(
        completed(service.get(&key("private-account"), &cx)),
        SecretAccess::Locked
    ));
    provider.mode.store(2, Ordering::SeqCst);
    assert!(matches!(
        completed(service.get(&key("private-account"), &cx)),
        SecretAccess::KeyInvalidated
    ));
    provider.mode.store(3, Ordering::SeqCst);
    let failure = service.get(&key("private-account"), &cx).err().unwrap();
    assert_eq!(failure.code, ErrorCode::PermissionDenied);
    let diagnostic = format!("{failure:?}");
    assert!(!diagnostic.contains("private-account"));
    assert!(!diagnostic.contains("private-app"));
    let missing = Secrets::with_provider("app", Arc::new(UnavailableCredentials)).unwrap();
    assert_eq!(
        missing.get(&key("a"), &cx).err().unwrap().code,
        ErrorCode::Unavailable
    );
}
#[test]
fn protection_requirement_and_late_cancellation_prevent_writes() {
    let provider = Arc::new(Fake::default());
    let service = Secrets::with_provider("app", provider.clone()).unwrap();
    let cx = OperationContext::default();
    assert_eq!(
        service
            .put(
                &key("a"),
                SecretBytes::new(vec![1]).unwrap(),
                SecretPolicy {
                    require_user_presence: true,
                    ..Default::default()
                },
                &cx
            )
            .err()
            .unwrap()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    *provider.cancel_policy.lock().unwrap() = Some(cx.cancellation.clone());
    assert!(matches!(
        service.put(
            &key("a"),
            SecretBytes::new(vec![1]).unwrap(),
            SecretPolicy::default(),
            &cx
        ),
        Ok(Completion::Cancelled(_))
    ));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
}
#[test]
fn partial_cleanup_reports_progress_and_rejects_foreign_enumeration() {
    let provider = Arc::new(Fake::default());
    let service = Secrets::with_provider("app", provider.clone()).unwrap();
    let cx = OperationContext::default();
    for purpose in ["a", "b"] {
        available(service.put(
            &CredentialKey::new("account", "service", purpose).unwrap(),
            SecretBytes::new(vec![1]).unwrap(),
            SecretPolicy::default(),
            &cx,
        ));
    }
    provider.mode.store(4, Ordering::SeqCst);
    let receipt = completed(service.remove_account("account", &cx));
    assert_eq!(receipt.deleted, 1);
    assert_eq!(
        receipt.state,
        CleanupState::Failed(ErrorCode::PlatformFailure)
    );
    provider.mode.store(0, Ordering::SeqCst);
    assert_eq!(completed(service.remove_account("account", &cx)).deleted, 1);
    provider.mode.store(5, Ordering::SeqCst);
    assert_eq!(
        service.remove_account("account", &cx).err().unwrap().code,
        ErrorCode::IntegrityFailure
    );
}
#[test]
fn identity_bounds_size_and_ui_context_are_checked() {
    assert!(CredentialKey::new("", "s", "p").is_err());
    assert!(CredentialKey::new("a\0b", "s", "p").is_err());
    assert!(SecretBytes::new(vec![0; MAX_SECRET_BYTES + 1]).is_err());
    let owner = gpui_system::UiOwner::new(gpui_system::SurfaceId(1));
    let cx = OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    let service = Secrets::with_provider("app", Arc::new(Fake::default())).unwrap();
    assert_eq!(
        service.get(&key("a"), &cx).err().unwrap().code,
        ErrorCode::InvalidInput
    );
}
