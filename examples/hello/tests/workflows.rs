use gpui_box_example_hello::workflows::{GalleryRoute, GalleryWorkbench};
use gpui_local_auth::{AccountUnlockStatus, VerificationPolicy, VerifiedFactor};

const REVIEWED_EDIT: &str = "纪要 · 已审阅 ✅";
const SECRET_CANARY: &str = "gpui-box-example-secret-canary";

#[test]
fn workflow_one_opens_edits_saves_shares_and_exposes_explicit_clipboard_fallback() {
    let mut workbench = GalleryWorkbench::new().expect("deterministic workbench");
    assert_eq!(workbench.document_draft(), "Initial reviewed draft");
    assert_eq!(workbench.current_route(), &GalleryRoute::Overview);

    assert!(
        workbench.copy_export_fallback().is_err(),
        "the fallback must reject a request before an export is staged"
    );
    assert!(workbench.fallback_status().contains("stage an export"));

    workbench.open_and_edit_document().expect("open and edit");
    assert!(workbench.document_draft().contains(REVIEWED_EDIT));
    workbench.save_atomically().expect("atomic save");
    assert!(workbench.save_status().contains("Atomic save"));
    assert!(workbench.save_status().contains("bytes"));

    workbench.stage_and_share().expect("native share");
    let staged_snapshot = workbench.document_draft().to_owned();
    assert!(workbench.share_status().contains("Native share submitted"));
    assert!(workbench.staged_export_active());
    assert!(workbench.share_handoff_retained());
    assert_eq!(
        workbench.staged_export_bytes(),
        workbench.document_draft().len() as u64
    );

    workbench.toggle_share_unsupported();
    assert!(!workbench.share_native_supported());
    workbench
        .stage_and_share()
        .expect("unsupported share is a reported outcome");
    assert!(workbench.share_status().contains("unsupported"));
    assert!(workbench.staged_export_active());
    assert!(
        workbench.clipboard_contents().is_none(),
        "the workflow must not silently fall back"
    );

    workbench
        .copy_export_fallback()
        .expect("explicit clipboard fallback");
    assert_eq!(
        workbench.clipboard_contents().as_deref(),
        Some(staged_snapshot.as_str())
    );
    assert!(
        workbench
            .fallback_status()
            .contains("Explicit clipboard fallback")
    );
    assert!(workbench.fallback_status().contains("lease retained"));
    assert!(workbench.staged_export_active());
    assert!(!workbench.share_status().contains("notes/"));

    workbench.set_draft("un-staged draft that must not reach the clipboard");
    assert_ne!(workbench.document_draft(), staged_snapshot);
    workbench
        .copy_export_fallback()
        .expect("fallback still copies the staged snapshot");
    assert_eq!(
        workbench.clipboard_contents().as_deref(),
        Some(staged_snapshot.as_str())
    );
    assert_ne!(
        workbench.clipboard_contents().as_deref(),
        Some(workbench.document_draft())
    );
}

#[test]
fn workflow_two_posts_replaces_and_routes_warm_and_cold_activation_without_duplicates() {
    let mut workbench = GalleryWorkbench::new().expect("deterministic workbench");
    assert!(
        workbench.replace_notification().is_err(),
        "replacement must be rejected before the first post"
    );
    assert!(workbench.notification_status().contains("post"));
    workbench
        .export_and_post_notification()
        .expect("export and post");
    assert!(workbench.notification_status().contains("posted"));
    assert!(workbench.notification_export_bytes() > 0);
    assert!(
        workbench
            .permission_provider()
            .current_status(gpui_notifications::PermissionKind::Notifications)
            .allows_use()
    );

    workbench
        .replace_notification()
        .expect("replace notification");
    assert!(workbench.notification_status().contains("replaced"));
    assert_eq!(workbench.notification_provider().replacement_ids().len(), 1);

    workbench
        .trigger_warm_activation()
        .expect("warm activation");
    assert_eq!(
        workbench.current_route(),
        &GalleryRoute::Document {
            document_id: "reviewed-plan".into()
        }
    );
    assert_eq!(workbench.navigation().depth(), 1);
    assert!(workbench.activation_status().contains("routed"));
    assert_eq!(workbench.handled_activation_count(), 1);

    workbench
        .trigger_warm_activation()
        .expect("duplicate warm activation");
    assert!(workbench.activation_status().contains("Duplicate"));
    assert_eq!(workbench.navigation().depth(), 1);

    workbench
        .simulate_cold_launch_activation()
        .expect("cold launch activation");
    assert_eq!(
        workbench.current_route(),
        &GalleryRoute::Document {
            document_id: "reviewed-plan".into()
        }
    );
    assert_eq!(workbench.navigation().depth(), 1);
    assert!(workbench.activation_status().contains("cold launch"));
    assert_eq!(
        workbench.handled_activation_count(),
        1,
        "cold launch must start with fresh process-local activation memory"
    );

    workbench
        .trigger_warm_activation()
        .expect("warm activation remains a durable duplicate");
    assert!(workbench.activation_status().contains("Duplicate"));
    assert_eq!(workbench.navigation().depth(), 1);
    assert_eq!(
        workbench.handled_activation_count(),
        1,
        "the durable tombstone suppresses the warm tap even after cold launch"
    );

    workbench.reset_route_to_root();
    assert_eq!(workbench.current_route(), &GalleryRoute::Overview);
    assert_eq!(workbench.navigation().depth(), 0);
}

#[test]
fn workflow_three_probes_policy_modes_unlocks_relocks_and_redacts_secret_metadata() {
    let mut workbench = GalleryWorkbench::new().expect("deterministic workbench");
    workbench.probe_local_auth().expect("availability probe");
    assert!(workbench.local_auth_status().contains("Available"));

    assert!(
        workbench.simulate_cancelled_verification().is_err(),
        "a cancelled prompt is reported as an error"
    );
    assert!(workbench.unlock_status().contains("UserCancelled"));
    assert!(matches!(
        workbench.unlock_state(),
        AccountUnlockStatus::Locked { .. }
    ));
    assert!(
        workbench.release_protected_secret().is_err(),
        "secrets must not be released while the session is locked"
    );
    assert!(workbench.secret_status().contains("gallery-vault-key"));
    assert!(workbench.secret_status().contains("locked"));
    assert!(!workbench.secret_status().contains(SECRET_CANARY));

    workbench
        .set_verification_policy(VerificationPolicy::BiometricOnly)
        .expect("biometric-only policy");
    workbench
        .verify_and_unlock_session()
        .expect("policy-restricted verification");
    assert!(workbench.unlock_status().contains("PolicyFallbackRequired"));
    assert!(matches!(
        workbench.unlock_state(),
        AccountUnlockStatus::Locked { .. }
    ));

    workbench
        .set_verification_policy(VerificationPolicy::BiometricOrDeviceCredential)
        .expect("fallback policy");
    workbench
        .verify_and_unlock_session()
        .expect("fallback verification");
    assert!(
        workbench
            .unlock_status()
            .contains("DeviceCredentialFallback")
    );
    assert!(matches!(
        workbench.unlock_state(),
        AccountUnlockStatus::Unlocked {
            factor: VerifiedFactor::DeviceCredentialFallback,
            ..
        }
    ));

    workbench
        .release_protected_secret()
        .expect("protected secret release");
    let length = SECRET_CANARY.len();
    assert!(workbench.secret_status().contains("gallery-vault-key"));
    assert!(
        workbench
            .secret_status()
            .contains(&format!("{length} bytes"))
    );
    assert!(workbench.secret_status().contains("released"));
    assert!(!workbench.secret_status().contains(SECRET_CANARY));

    workbench
        .simulate_background_relock()
        .expect("background relock");
    assert!(workbench.relock_status().contains("AppBackgrounded"));
    assert!(matches!(
        workbench.unlock_state(),
        AccountUnlockStatus::Locked { .. }
    ));
    assert!(
        workbench.release_protected_secret().is_err(),
        "background relock must gate protected-secret release"
    );
    assert!(workbench.secret_status().contains("locked"));
    assert!(!workbench.secret_status().contains(SECRET_CANARY));

    workbench
        .simulate_session_lock()
        .expect("session-lock relock");
    assert!(workbench.relock_status().contains("SessionLocked"));
    assert!(matches!(
        workbench.unlock_state(),
        AccountUnlockStatus::Locked { .. }
    ));
    assert!(
        workbench.release_protected_secret().is_err(),
        "session lock must gate protected-secret release"
    );
    assert!(workbench.secret_status().contains("locked"));
    assert!(!workbench.secret_status().contains(SECRET_CANARY));
}
