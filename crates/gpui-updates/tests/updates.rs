use ed25519_dalek::{Signer, SigningKey};
use gpui_app_data::{AppData, DirectoryRoots};
use gpui_system::{CancelReason, Completion, ErrorCode, OperationContext, SurfaceId, UiOwner};
use gpui_updates::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor};

const NOW: u64 = 1_800_000_000;
const ARTIFACT: &[u8] = b"signed APK fixture bytes";

fn value<T>(result: gpui_system::ServiceResult<T>) -> T {
    match result.unwrap() {
        Completion::Completed(value) => value,
        _ => panic!("unexpected cancellation"),
    }
}
fn installed() -> InstalledIdentity {
    InstalledIdentity {
        application_id: "dev.example.app".into(),
        version_code: 1,
        signer_sha256: "ab".repeat(32),
        android_api: 35,
        abi: "x86_64".into(),
    }
}
fn policy() -> UpdateTrustPolicy {
    UpdateTrustPolicy {
        installed: installed(),
        keys: BTreeMap::from([(
            "release-1".into(),
            SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes(),
        )]),
        artifact_origins: vec!["https://releases.example.com".into()],
        maximum_artifact_bytes: 1024 * 1024,
    }
}
fn metadata() -> UpdateMetadata {
    UpdateMetadata {
        schema_version: 1,
        channel: UpdateChannel::AndroidDirectApk,
        application_id: "dev.example.app".into(),
        version: "0.2.0".into(),
        version_code: 2,
        sequence: 10,
        issued_at: NOW - 60,
        expires_at: NOW + 3600,
        artifact: ArtifactMetadata {
            url: "https://releases.example.com/app.apk".into(),
            sha256: format!("{:x}", Sha256::digest(ARTIFACT)),
            size: ARTIFACT.len() as u64,
            signer_sha256: "ab".repeat(32),
            abis: vec!["arm64-v8a".into(), "x86_64".into()],
            minimum_api: 35,
            maximum_api: None,
        },
    }
}
fn envelope_with_payload(payload: String, key: &SigningKey) -> Vec<u8> {
    let mut message = SIGNATURE_DOMAIN.to_vec();
    message.extend_from_slice(payload.as_bytes());
    let signature = key
        .sign(&message)
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    serde_json::to_vec(&SignedMetadata {
        schema_version: 1,
        key_id: "release-1".into(),
        payload,
        signature,
    })
    .unwrap()
}
fn envelope(metadata: &UpdateMetadata) -> Vec<u8> {
    envelope_with_payload(
        serde_json::to_string(metadata).unwrap(),
        &SigningKey::from_bytes(&[7; 32]),
    )
}
fn setup() -> (tempfile::TempDir, AppData, UpdateVerifier) {
    let directory = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(
        directory.path().join("update 有 空格"),
    ))
    .unwrap();
    let verifier = UpdateVerifier::open(policy(), &app).unwrap();
    (directory, app, verifier)
}

#[test]
fn exact_signed_payload_and_artifact_digest_are_verified() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    let update = value(verifier.verify(&envelope(&metadata()), NOW, &context));
    assert_eq!(update.metadata().version_code, 2);
    value(update.verify_artifact(Cursor::new(ARTIFACT), &context));
    for bytes in [
        b"modified APK".as_slice(),
        &ARTIFACT[..ARTIFACT.len() - 1],
        b"signed APK fixture bytes extra",
    ] {
        assert_eq!(
            update
                .verify_artifact(Cursor::new(bytes), &context)
                .unwrap_err()
                .code,
            ErrorCode::IntegrityFailure
        );
    }
    let bytes = envelope(&metadata());
    let mut signed: SignedMetadata = serde_json::from_slice(&bytes).unwrap();
    signed.payload.push(' ');
    assert_eq!(
        verifier
            .verify(&serde_json::to_vec(&signed).unwrap(), NOW, &context)
            .unwrap_err()
            .code,
        ErrorCode::IntegrityFailure
    );
    let wrong = envelope_with_payload(
        serde_json::to_string(&metadata()).unwrap(),
        &SigningKey::from_bytes(&[8; 32]),
    );
    assert_eq!(
        verifier.verify(&wrong, NOW, &context).unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
}

#[test]
fn signed_wrong_identity_version_signer_platform_origin_and_time_are_rejected() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    let changes: Vec<fn(&mut UpdateMetadata)> = vec![
        |m| m.application_id = "dev.other.app".into(),
        |m| m.version_code = 1,
        |m| m.version_code = 2_100_000_001,
        |m| m.channel = UpdateChannel::AndroidPlay,
        |m| m.artifact.signer_sha256 = "cd".repeat(32),
        |m| m.artifact.minimum_api = 36,
        |m| m.artifact.maximum_api = Some(34),
        |m| m.artifact.abis = vec!["armeabi-v7a".into()],
        |m| m.artifact.abis = vec!["x86_64".into(), "x86_64".into()],
        |m| m.artifact.size = 0,
        |m| m.artifact.size = 2 * 1024 * 1024,
        |m| m.artifact.sha256 = "AA".repeat(32),
        |m| m.artifact.url = "http://releases.example.com/app.apk".into(),
        |m| m.artifact.url = "https://releases.example.com.evil.test/app.apk".into(),
        |m| m.artifact.url = "https://releases.example.com:444/app.apk".into(),
        |m| m.artifact.url = "https://user:secret@releases.example.com/app.apk".into(),
        |m| m.artifact.url = "https://releases.example.com/app.apk#fragment".into(),
        |m| m.expires_at = NOW,
        |m| m.issued_at = NOW + 301,
        |m| m.expires_at = NOW + 8 * 24 * 3600,
        |m| m.schema_version = 2,
        |m| m.sequence = 0,
    ];
    for change in changes {
        let mut metadata = metadata();
        change(&mut metadata);
        assert_eq!(
            verifier
                .verify(&envelope(&metadata), NOW, &context)
                .unwrap_err()
                .code,
            ErrorCode::IntegrityFailure,
            "{metadata:?}"
        );
    }
}

#[test]
fn envelope_bounds_duplicate_fields_unknown_keys_and_key_pins_are_strict() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    assert_eq!(
        verifier
            .verify(&vec![b' '; MAX_ENVELOPE_BYTES + 1], NOW, &context)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    let good = envelope(&metadata());
    let signed: serde_json::Value = serde_json::from_slice(&good).unwrap();
    for (field, value) in [
        ("key_id", serde_json::json!("unknown")),
        ("signature", serde_json::json!("00")),
        ("schema_version", serde_json::json!(2)),
        (
            "payload",
            serde_json::json!("x".repeat(MAX_PAYLOAD_BYTES + 1)),
        ),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut invalid = signed.clone();
        invalid[field] = value;
        assert!(
            verifier
                .verify(&serde_json::to_vec(&invalid).unwrap(), NOW, &context)
                .is_err()
        );
    }
    let duplicate =
        serde_json::to_string(&metadata())
            .unwrap()
            .replacen("{", "{\"version_code\":3,", 1);
    let duplicate = envelope_with_payload(duplicate, &SigningKey::from_bytes(&[7; 32]));
    assert!(verifier.verify(&duplicate, NOW, &context).is_err());
    let mut policy = policy();
    policy.keys.insert("weak".into(), [0; 32]);
    assert!(UpdateVerifier::open(policy, &_app).is_err());
}

#[test]
fn checkpoint_survives_reopen_and_rejects_replays_equivocation_and_clock_rollback() {
    let (_directory, app, verifier) = setup();
    let context = OperationContext::default();
    let original = metadata();
    value(verifier.verify(&envelope(&original), NOW, &context));
    assert_eq!(
        UpdateVerifier::open(policy(), &app).err().unwrap().code,
        ErrorCode::Busy
    );
    drop(verifier);
    let verifier = UpdateVerifier::open(policy(), &app).unwrap();
    value(verifier.verify(&envelope(&original), NOW + 1, &context));
    let mut changed = original.clone();
    changed.version = "0.2.1".into();
    assert!(
        verifier
            .verify(&envelope(&changed), NOW + 1, &context)
            .is_err()
    );
    changed.sequence = 9;
    assert!(
        verifier
            .verify(&envelope(&changed), NOW + 1, &context)
            .is_err()
    );
    changed.sequence = 11;
    let current = value(verifier.verify(&envelope(&changed), NOW + 2, &context));
    assert!(
        verifier
            .verify(&envelope(&original), NOW + 2, &context)
            .is_err()
    );
    assert!(
        verifier
            .verify(&envelope(&changed), NOW - 301, &context)
            .is_err()
    );
    verifier.revalidate(&current, NOW + 3).unwrap();
    assert!(verifier.revalidate(&current, original.expires_at).is_err());
    drop(verifier);
    let mut installed_policy = policy();
    installed_policy.installed.version_code = 2;
    let verifier = UpdateVerifier::open(installed_policy, &app).unwrap();
    assert!(
        verifier
            .verify(&envelope(&changed), NOW + 3, &context)
            .is_err()
    );
}

#[test]
fn cancellation_does_not_advance_trust_checkpoint() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    context.cancellation.cancel(CancelReason::Caller);
    assert!(matches!(
        verifier
            .verify(&envelope(&metadata()), NOW, &context)
            .unwrap(),
        Completion::Cancelled(CancelReason::Caller)
    ));
    let update = value(verifier.verify(&envelope(&metadata()), NOW, &OperationContext::default()));
    assert!(matches!(
        update
            .verify_artifact(Cursor::new(ARTIFACT), &context)
            .unwrap(),
        Completion::Cancelled(CancelReason::Caller)
    ));
    let owner = UiOwner::new(SurfaceId(2));
    let ui = OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    assert_eq!(
        verifier
            .verify(&envelope(&metadata()), NOW, &ui)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn stale_foreign_callbacks_and_progress_cannot_complete_a_new_attempt() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    let update = value(verifier.verify(&envelope(&metadata()), NOW, &context));
    let mut controller = UpdateController::new(installed());
    let first = controller.begin_check().unwrap();
    assert!(controller.cancel(CancelReason::Caller));
    let second = controller.begin_check().unwrap();
    assert!(!controller.checked(first, Some(update.clone())).unwrap());
    let mut other = UpdateController::new(installed());
    let foreign = other.begin_check().unwrap();
    assert!(!controller.checked(foreign, Some(update.clone())).unwrap());
    assert!(controller.checked(second, Some(update.clone())).unwrap());
    let download = controller.begin_download().unwrap();
    assert!(!controller.progress(second, 1).unwrap());
    assert!(controller.progress(download, 5).unwrap());
    assert!(controller.progress(download, 4).is_err());
    assert!(controller.progress(download, 99999).is_err());
    let receipt = value(update.verify_artifact(Cursor::new(ARTIFACT), &context));
    assert!(controller.artifact_verified(download, receipt).unwrap());
    assert!(!controller.fail(download, ErrorCode::PlatformFailure, true));
    assert!(!controller.progress(download, 10).unwrap());
}

#[test]
fn installation_requires_live_ui_fresh_offer_and_installed_version_confirmation() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    let update = value(verifier.verify(&envelope(&metadata()), NOW, &context));
    let mut controller = UpdateController::new(installed());
    let check = controller.begin_check().unwrap();
    controller.checked(check, Some(update.clone())).unwrap();
    let download = controller.begin_download().unwrap();
    controller
        .artifact_verified(
            download,
            value(update.verify_artifact(Cursor::new(ARTIFACT), &context)),
        )
        .unwrap();
    assert!(
        controller
            .begin_installer_handoff(&verifier, NOW, &context)
            .is_err()
    );
    let owner = UiOwner::new(SurfaceId(1));
    let ui = OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    owner.advance_generation().unwrap();
    assert!(
        controller
            .begin_installer_handoff(&verifier, NOW, &ui)
            .is_err()
    );
    let ui = OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    let ticket = value(controller.begin_installer_handoff(&verifier, NOW + 1, &ui));
    assert!(!controller.cancel(CancelReason::Caller));
    assert!(controller.begin_check().is_err());
    assert!(controller.installer_handoff(ticket, true).unwrap().accepted);
    assert!(!controller.confirm_installed(&installed()).unwrap());
    let mut current = installed();
    current.version_code = 2;
    current.signer_sha256 = "cd".repeat(32);
    assert!(controller.confirm_installed(&current).is_err());
    current.signer_sha256 = "ab".repeat(32);
    assert!(controller.confirm_installed(&current).unwrap());
    assert_eq!(
        controller.state(),
        &UpdateState::Completed { version_code: 2 }
    );
    assert!(controller.installer_handoff(ticket, true).is_none());
    let retry = controller.begin_check().unwrap();
    assert!(controller.checked(retry, Some(update)).is_err());
}

#[test]
fn newer_signed_offer_invalidates_old_handoff_and_native_decline_allows_retry() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    let old = value(verifier.verify(&envelope(&metadata()), NOW, &context));
    let mut newer = metadata();
    newer.sequence = 11;
    newer.version_code = 3;
    newer.version = "0.3.0".into();
    let newer = value(verifier.verify(&envelope(&newer), NOW + 1, &context));
    assert!(verifier.revalidate(&old, NOW + 1).is_err());
    let mut controller = UpdateController::new(installed());
    let check = controller.begin_check().unwrap();
    controller.checked(check, Some(newer.clone())).unwrap();
    let download = controller.begin_download().unwrap();
    controller
        .artifact_verified(
            download,
            value(newer.verify_artifact(Cursor::new(ARTIFACT), &context)),
        )
        .unwrap();
    let owner = UiOwner::new(SurfaceId(1));
    let ui = OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    let ticket = value(controller.begin_installer_handoff(&verifier, NOW + 1, &ui));
    controller.installer_handoff(ticket, true).unwrap();
    assert!(controller.installer_declined(ticket, CancelReason::User));
    assert!(controller.begin_check().is_ok());
    assert!(!controller.installer_declined(ticket, CancelReason::User));
}

#[test]
fn provider_contracts_do_not_claim_installed_native_adapters() {
    let manifest = provider_manifest(
        UpdatePlatform::Android,
        UpdateChannel::AndroidDirectApk,
        UpdateOperation::InstallerHandoff,
    );
    assert!(!manifest.native_provider_installed);
    assert_eq!(manifest.evidence, gpui_system::Evidence::ContractOnly);
}

#[test]
fn retry_cancellation_and_late_ui_callbacks_preserve_install_reconciliation() {
    let (_directory, _app, verifier) = setup();
    let context = OperationContext::default();
    let update = value(verifier.verify(&envelope(&metadata()), NOW, &context));
    let mut controller = UpdateController::new(installed());
    let failed = controller.begin_check().unwrap();
    assert!(controller.fail(failed, ErrorCode::Unavailable, true));
    let check = controller.begin_check().unwrap();
    assert!(!controller.fail(failed, ErrorCode::PlatformFailure, false));
    controller.checked(check, Some(update.clone())).unwrap();
    let download = controller.begin_download().unwrap();
    controller
        .artifact_verified(
            download,
            value(update.verify_artifact(Cursor::new(ARTIFACT), &context)),
        )
        .unwrap();
    let owner = UiOwner::new(SurfaceId(3));
    let ui = OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    let ticket = value(controller.begin_installer_handoff(&verifier, NOW + 1, &ui));
    owner.advance_generation().unwrap();
    assert!(controller.installer_handoff(ticket, true).is_none());
    let mut current = installed();
    current.version_code = 2;
    assert!(controller.confirm_installed(&current).unwrap());
    assert_eq!(
        controller.state(),
        &UpdateState::Completed { version_code: 2 }
    );
}

#[test]
fn corrupt_checkpoint_is_preserved_and_rejected() {
    let (_directory, app, verifier) = setup();
    drop(verifier);
    let store = app
        .settings(
            gpui_app_data::DataScope::Application,
            "update-trust",
            1,
            8192,
        )
        .unwrap();
    value(store.save(&serde_json::json!({"application_id":"", "sequence":10, "payload_sha256":"", "version_code":0, "observed_time":0}), &OperationContext::default()));
    assert_eq!(
        UpdateVerifier::open(policy(), &app).err().unwrap().code,
        ErrorCode::IntegrityFailure
    );
    assert!(value(store.load::<serde_json::Value>(&OperationContext::default())).is_some());
}

#[test]
fn held_offer_cannot_survive_removal_of_its_pinned_signing_key() {
    let (_directory, app, verifier) = setup();
    let update = value(verifier.verify(&envelope(&metadata()), NOW, &OperationContext::default()));
    drop(verifier);
    let mut changed = policy();
    changed.keys.insert(
        "release-1".into(),
        SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes(),
    );
    let verifier = UpdateVerifier::open(changed, &app).unwrap();
    assert_eq!(
        verifier.revalidate(&update, NOW + 1).unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
}
