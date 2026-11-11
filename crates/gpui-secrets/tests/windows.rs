#![cfg(target_os = "windows")]
use gpui_secrets::*;
use gpui_system::{Completion, OperationContext, ServiceResult};
use std::{
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
fn available<T>(result: ServiceResult<SecretAccess<T>>) -> T {
    match result.unwrap() {
        Completion::Completed(SecretAccess::Available(value)) => value,
        _ => panic!("Windows credential provider did not accept the test operation"),
    }
}
fn key(account: &str) -> CredentialKey {
    CredentialKey::new(account, "scratch-service", "fixture").unwrap()
}
struct Scratch(Secrets);
impl Drop for Scratch {
    fn drop(&mut self) {
        for account in ["account-a", "account-ab"] {
            let _ = self.0.remove_account(account, &OperationContext::default());
        }
    }
}
#[test]
fn native_child_reopen() {
    if let Some(app) = std::env::var_os("GPUI_SECRETS_TEST_APP") {
        let service = Secrets::native(app.to_str().unwrap()).unwrap();
        let secret = available(service.get(&key("account-a"), &OperationContext::default()))
            .expect("scratch fixture must survive process reopening");
        assert!(secret.expose() == b"updated-test-fixture");
    }
}
#[test]
fn windows_native_roundtrip_reopen_process_update_and_account_cleanup() {
    let app = format!(
        "dev.gpui-box.test.credentials.{}.{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let scratch = Scratch(Secrets::native(&app).unwrap());
    let cx = OperationContext::default();
    available(scratch.0.put(
        &key("account-a"),
        SecretBytes::new(b"initial-test-fixture".to_vec()).unwrap(),
        SecretPolicy::default(),
        &cx,
    ));
    available(scratch.0.put(
        &key("account-ab"),
        SecretBytes::new(b"other-account-fixture".to_vec()).unwrap(),
        SecretPolicy::default(),
        &cx,
    ));
    available(scratch.0.put(
        &key("account-a"),
        SecretBytes::new(b"updated-test-fixture".to_vec()).unwrap(),
        SecretPolicy::default(),
        &cx,
    ));
    let reopened = Secrets::native(&app).unwrap();
    let value = available(reopened.get(&key("account-a"), &cx)).unwrap();
    assert!(value.expose() == b"updated-test-fixture");
    drop(value);
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "native_child_reopen", "--nocapture"])
        .env("GPUI_SECRETS_TEST_APP", &app)
        .status()
        .unwrap();
    assert!(child.success());
    let receipt = match scratch.0.remove_account("account-a", &cx).unwrap() {
        Completion::Completed(receipt) => receipt,
        _ => panic!("unexpected cancellation"),
    };
    assert_eq!(receipt.deleted, 1);
    assert_eq!(receipt.state, CleanupState::Complete);
    assert!(available(reopened.get(&key("account-a"), &cx)).is_none());
    assert!(available(reopened.get(&key("account-ab"), &cx)).is_some());
    assert!(available(reopened.delete(&key("account-ab"), &cx)).existed);
    assert!(!available(reopened.delete(&key("account-ab"), &cx)).existed);
}
