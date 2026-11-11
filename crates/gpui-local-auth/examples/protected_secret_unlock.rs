//! Capability-gated protected-secret workflow.
//!
//! This example composes `gpui-local-auth`, `gpui-secrets` and `gpui-lifecycle`
//! without showing a native prompt. It demonstrates account-scoped ciphertext
//! storage, local unlock policy, relocking on background and `KeyInvalidated`
//! recovery after a biometric enrollment change.

use gpui_lifecycle::{LifecycleService, ProcessId, SessionId};
use gpui_local_auth::{
    AccountId, AccountUnlockManager, AccountUnlockStatus, AppPhase, ChallengeBytes, Completion,
    Configuration, FakeLocalAuthProvider, FakeVerificationMode, LinuxLocalAuthConfig,
    LocalAuthHostState, LocalAuthOperation, LocalAuthProvider, LocalAuthService,
    LocalUnlockFlowModel, LocalUnlockFlowState, LockReason, OperationContext, ProtectedKeyId,
    ProtectedSecretOutcome, ProtectedSecretRequest, ProtectedSignatureOutcome, Request,
    SignatureAlgorithm, SignatureRequest, SurfaceId, SystemError, UiOwner,
    UnavailableLocalAuthProvider, UnlockPolicy, VerificationOutcome, VerificationPolicy,
    VerificationPurpose,
};
use gpui_secrets::{
    CredentialKey, CredentialProvider, DeleteReceipt, ProviderResult, SecretAccess, SecretBytes,
    SecretOperation, SecretPolicy, Secrets,
};
use gpui_system::{Availability, Capability, Evidence, PermissionState, Support};
use std::{
    collections::BTreeMap,
    error::Error,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context as TaskContext, Poll, Waker},
    time::{Duration, Instant},
};

/// An in-memory `CredentialProvider` for the example. Production applications
/// must use `Secrets::native` or an audited platform provider.
#[derive(Default)]
struct MemoryCredentials {
    values: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl CredentialProvider for MemoryCredentials {
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
            constraints: Vec::new(),
            observed_revision: 1,
        })
        .collect()
    }

    fn validate_policy(&self, _policy: SecretPolicy) -> Result<(), SystemError> {
        Ok(())
    }

    fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()> {
        self.values
            .lock()
            .expect("memory credential store")
            .insert(target.to_owned(), value.expose().to_vec());
        Ok(SecretAccess::Available(()))
    }

    fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>> {
        let bytes = self
            .values
            .lock()
            .expect("memory credential store")
            .get(target)
            .cloned();
        match bytes {
            Some(bytes) => Ok(SecretAccess::Available(Some(
                SecretBytes::new(bytes).expect("bounded memory secret"),
            ))),
            None => Ok(SecretAccess::Available(None)),
        }
    }

    fn delete(&self, target: &str) -> ProviderResult<DeleteReceipt> {
        let existed = self
            .values
            .lock()
            .expect("memory credential store")
            .remove(target)
            .is_some();
        Ok(SecretAccess::Available(DeleteReceipt { existed }))
    }

    fn list(&self, prefix: &str) -> ProviderResult<Vec<String>> {
        let targets: Vec<String> = self
            .values
            .lock()
            .expect("memory credential store")
            .keys()
            .filter(|target| target.starts_with(prefix))
            .cloned()
            .collect();
        Ok(SecretAccess::Available(targets))
    }
}

fn completed<T>(mut request: Request<T>) -> Result<T, Box<dyn Error>> {
    match Pin::new(&mut request).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(Ok(Completion::Completed(value))) => Ok(value),
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            Err(format!("Example operation was cancelled: {reason:?}").into())
        }
        Poll::Ready(Err(error)) => Err(error.into()),
        Poll::Pending => Err("Example operation did not complete synchronously".into()),
    }
}

fn put_secret(
    secrets: &Secrets,
    key: &CredentialKey,
    bytes: &[u8],
    context: &OperationContext,
) -> Result<(), Box<dyn Error>> {
    let value = SecretBytes::new(bytes.to_vec())?;
    match secrets.put(key, value, SecretPolicy::default(), context)? {
        Completion::Completed(SecretAccess::Available(())) => Ok(()),
        _ => Err("Example credential write did not complete".into()),
    }
}

fn get_secret(
    secrets: &Secrets,
    key: &CredentialKey,
    context: &OperationContext,
) -> Result<Vec<u8>, Box<dyn Error>> {
    match secrets.get(key, context)? {
        Completion::Completed(SecretAccess::Available(Some(secret))) => {
            Ok(secret.expose().to_vec())
        }
        _ => Err("Example credential is missing or unavailable".into()),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let lifecycle = LifecycleService::new(ProcessId(1), SessionId(1));
    lifecycle.set_phase_at(AppPhase::ForegroundActive, started)?;

    let surface = SurfaceId(1);
    let owner = UiOwner::new(surface);
    let background = OperationContext::default();
    let foreground = OperationContext {
        ui: Some(owner.context(None)?),
        ..Default::default()
    };

    let fake = Rc::new(FakeLocalAuthProvider::new());
    fake.set_available();
    fake.set_auto_verify(Some(FakeVerificationMode::Biometric));
    let service = LocalAuthService::new(fake.clone());
    service.reconcile_host_state(&LocalAuthHostState::foreground(surface, 1), started);

    println!("localhost authentication capabilities:");
    for capability in service.capabilities() {
        assert_eq!(capability.evidence, Evidence::ContractOnly);
        println!(
            "  {:?}: availability={:?} configuration={:?} evidence={:?}",
            capability.operation,
            capability.availability,
            capability.configuration,
            capability.evidence,
        );
    }

    let unavailable = UnavailableLocalAuthProvider::for_linux(LinuxLocalAuthConfig::Unconfigured);
    let linux = unavailable.capability(LocalAuthOperation::VerifyUser(
        VerificationPolicy::BiometricOnly,
    ));
    assert_eq!(linux.availability, Availability::Unavailable);
    assert_eq!(linux.configuration, Configuration::Required);
    assert_eq!(linux.evidence, Evidence::ContractOnly);
    println!(
        "linux contract: availability={:?} configuration={:?} evidence={:?}",
        linux.availability, linux.configuration, linux.evidence
    );

    // Account-scoped metadata and wrapped key material live in the credential
    // store, not in settings or logs.
    let credentials = Arc::new(MemoryCredentials::default());
    let secrets = Secrets::with_provider("gpui-local-auth-example", credentials)?;
    let alice = AccountId::new("alice")?;
    let bob = AccountId::new("bob")?;
    let metadata_key = CredentialKey::new(alice.as_str(), "local-auth-demo", "metadata")?;
    let ciphertext_key = CredentialKey::new(alice.as_str(), "local-auth-demo", "ciphertext")?;
    put_secret(&secrets, &metadata_key, b"vault metadata v1", &background)?;
    put_secret(&secrets, &ciphertext_key, b"wrapped-dek-v1", &background)?;
    let ciphertext = get_secret(&secrets, &ciphertext_key, &background)?;
    println!(
        "stored {} ciphertext bytes for the example account",
        ciphertext.len()
    );

    let protected_key = ProtectedKeyId::new("alice-vault-key")?;
    fake.seal_protected_secret(
        &alice,
        &protected_key,
        SecretBytes::new(ciphertext.clone())?,
    );

    let unlock_policy = UnlockPolicy::new(
        Duration::from_secs(60),
        Duration::from_secs(600),
        Duration::from_secs(30),
    )?;
    let mut flow = LocalUnlockFlowModel::new(
        alice.clone(),
        unlock_policy,
        VerificationPolicy::BiometricOrDeviceCredential,
    )?;
    flow.begin_availability_check();
    let snapshot =
        completed(service.query_availability(flow.verification_policy(), background.clone()))?;
    flow.apply_availability(&snapshot);
    assert_eq!(flow.state(), LocalUnlockFlowState::Locked);
    assert!(flow.begin_prompt());

    let purpose = VerificationPurpose::new("unlock vault")?;
    let outcome = completed(service.verify_user(
        flow.verification_request(purpose.clone()),
        foreground.clone(),
    ))?;
    flow.apply_outcome(&outcome);
    assert!(flow.is_unlocked());
    let receipt = match outcome {
        VerificationOutcome::Verified(receipt) => receipt,
        other => return Err(format!("expected verification, got {other:?}").into()),
    };

    let mut manager = AccountUnlockManager::new(*flow.unlock_policy())?;
    manager.set_active_account(Some(alice.clone()));
    let grant = manager.grant_unlock(&alice, &receipt, Instant::now())?;
    println!(
        "unlocked account with {:?}; grant expires at {:?}",
        grant.factor, grant.expires_at
    );
    assert!(matches!(
        manager.status(&bob, Instant::now()),
        AccountUnlockStatus::Locked {
            reason: LockReason::NeverUnlocked
        }
    ));
    println!("account isolation: bob remains locked");

    let unlocked = completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            protected_key.clone(),
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose.clone(),
        ),
        foreground.clone(),
    ))?;
    match unlocked {
        ProtectedSecretOutcome::Unlocked(secret) => {
            assert_eq!(secret.expose(), ciphertext.as_slice());
            println!(
                "protected secret released ({} bytes)",
                secret.expose().len()
            );
        }
        other => return Err(format!("protected secret was not released: {other:?}").into()),
    }

    let signing_key = ProtectedKeyId::new("alice-signing-key")?;
    fake.register_signing_key(&alice, &signing_key, SignatureAlgorithm::EcdsaP256Sha256);
    let signed = completed(service.sign_challenge(
        SignatureRequest::new(
            alice.clone(),
            signing_key,
            SignatureAlgorithm::EcdsaP256Sha256,
            ChallengeBytes::new(b"example challenge".to_vec())?,
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose.clone(),
        ),
        foreground.clone(),
    ))?;
    match signed {
        ProtectedSignatureOutcome::Signed(receipt) => {
            assert!(!receipt.signature.is_empty());
            println!("signed challenge with {:?}", receipt.algorithm);
        }
        other => return Err(format!("challenge was not signed: {other:?}").into()),
    }

    // Backgrounding relocks the account and invalidates any in-flight prompt.
    let paused_at = Instant::now();
    lifecycle.set_phase_at(AppPhase::Background, paused_at)?;
    let background_host = LocalAuthHostState::foreground(surface, 1).with_phase(lifecycle.phase());
    service.reconcile_host_state(&background_host, paused_at);
    manager.reconcile_host_state(&background_host, paused_at);
    assert!(matches!(
        manager.status(&alice, paused_at),
        AccountUnlockStatus::Locked {
            reason: LockReason::AppBackgrounded
        }
    ));
    println!("background transition relocked the account");

    lifecycle.set_phase_at(AppPhase::ForegroundActive, Instant::now())?;
    service.reconcile_host_state(&LocalAuthHostState::foreground(surface, 1), Instant::now());

    // Biometric re-enrollment invalidates the protected key material.
    fake.advance_enrollment_epoch();
    let invalidated = completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            protected_key.clone(),
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose.clone(),
        ),
        foreground.clone(),
    ))?;
    assert!(matches!(
        invalidated,
        ProtectedSecretOutcome::KeyInvalidated
    ));
    manager.invalidate_enrollment();
    assert!(matches!(
        manager.status(&alice, Instant::now()),
        AccountUnlockStatus::Locked {
            reason: LockReason::Invalidated
        }
    ));
    println!("enrollment change invalidated protected material");

    // Recovery: the app re-seals only after the user re-enrolls and verifies.
    fake.seal_protected_secret(&alice, &protected_key, SecretBytes::new(ciphertext)?);
    let recovered = completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice,
            protected_key,
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose,
        ),
        foreground,
    ))?;
    match recovered {
        ProtectedSecretOutcome::Unlocked(secret) => {
            println!(
                "recovered protected secret ({} bytes)",
                secret.expose().len()
            );
        }
        other => return Err(format!("recovery failed: {other:?}").into()),
    }

    Ok(())
}
