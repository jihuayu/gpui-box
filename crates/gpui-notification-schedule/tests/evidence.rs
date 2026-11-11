//! Strictly separated submission, presentation and activation evidence.

mod support;

use gpui_notification_schedule::*;
use std::collections::BTreeMap;
use support::*;

#[test]
fn submission_never_asserts_presentation_or_activation() {
    let directory = TestDirectory::new("evidence-submission");
    let fixture = Fixture::open(directory.path());
    let subscription = fixture.service.subscribe();
    let receipt = submit_after_delay(&fixture, "submit-only", 60_000);
    assert!(receipt.is_scheduled());
    assert_eq!(receipt.disposition, SubmissionDisposition::Created);
    let record = fixture
        .service
        .schedule(&id("submit-only"))
        .expect("record");
    assert_eq!(record.presented_count, 0);
    assert_eq!(record.last_presented_at_epoch_ms, None);
    assert!(record.delivery_observations.is_empty());
    assert_eq!(record.activation_count, 0);
    assert_eq!(record.last_activated_at_epoch_ms, None);
    assert!(record.activation_handoffs.is_empty());
    let events = drain_events(&subscription);
    assert!(matches!(
        events.as_slice(),
        [NotificationScheduleEvent::Submitted { .. }]
    ));
    assert!(!events.iter().any(|event| matches!(
        event,
        NotificationScheduleEvent::Presented { .. } | NotificationScheduleEvent::Activated { .. }
    )));
}

#[test]
fn presentation_requires_capability_and_never_records_activation() {
    let directory = TestDirectory::new("evidence-presentation");
    let fixture = Fixture::open(directory.path());
    let receipt = submit_after_delay(&fixture, "present", 60_000);
    let observation = DeliveryObservation::new(
        id("present"),
        receipt.schedule_epoch,
        receipt.next_fire_at_epoch_ms.expect("fire"),
    );

    fixture.provider.set_support(
        ScheduleOperation::ObserveScheduledDelivery,
        Support::Unsupported,
    );
    let error = fixture
        .service
        .record_delivery_observation(&OperationContext::default(), observation.clone())
        .expect_err("capability gate");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        fixture
            .service
            .schedule(&id("present"))
            .expect("record")
            .presented_count,
        0
    );

    fixture
        .provider
        .set_support(ScheduleOperation::ObserveScheduledDelivery, Support::Native);
    let recorded = fixture
        .service
        .record_delivery_observation(&OperationContext::default(), observation)
        .expect("presented");
    assert_eq!(recorded.presented_count, 1);
    let record = fixture.service.schedule(&id("present")).expect("record");
    assert_eq!(
        record.last_presented_at_epoch_ms,
        recorded.presented_at_epoch_ms.into()
    );
    assert_eq!(record.activation_count, 0);
    assert_eq!(record.last_activated_at_epoch_ms, None);
    assert!(record.activation_handoffs.is_empty());
}

#[test]
fn activation_never_fabricates_presentation() {
    let directory = TestDirectory::new("evidence-activation");
    let fixture = Fixture::open(directory.path());
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("actionable"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                actionable_content(),
            ),
        )
        .expect("submitted");
    let handoff = fixture
        .service
        .record_activation(
            &OperationContext::default(),
            ActivationObservation::tap(
                id("actionable"),
                receipt.schedule_epoch,
                receipt.submitted_at_epoch_ms + 1_000,
            ),
        )
        .expect("activation");
    assert_eq!(handoff.source, ActivationSource::Notification);
    assert!(handoff.target.is_some());
    assert!(handoff.payload.is_some());
    assert!(handoff.action_id.is_none());
    assert_eq!(handoff.action_label(&actionable_content()), None);
    assert!(
        handoff
            .into_activation_event(std::time::SystemTime::now())
            .is_ok()
    );

    let record = fixture.service.schedule(&id("actionable")).expect("record");
    assert_eq!(record.presented_count, 0);
    assert_eq!(record.last_presented_at_epoch_ms, None);
    assert!(record.delivery_observations.is_empty());
    assert_eq!(record.activation_count, 1);
    assert_eq!(
        record.last_activated_at_epoch_ms,
        Some(receipt.submitted_at_epoch_ms + 1_000)
    );
    assert_eq!(record.activation_handoffs.len(), 1);
}

#[test]
fn declared_actions_and_text_input_eligibility_are_validated() {
    let directory = TestDirectory::new("evidence-actions");
    let fixture = Fixture::open(directory.path());
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("actionable"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                actionable_content(),
            ),
        )
        .expect("submitted");
    let now = receipt.submitted_at_epoch_ms + 1_000;
    let error = fixture
        .service
        .record_activation(
            &OperationContext::default(),
            ActivationObservation::action(
                id("actionable"),
                receipt.schedule_epoch,
                ActionId::new("undeclared").expect("action id"),
                now,
            ),
        )
        .expect_err("undeclared action");
    assert_eq!(error.code, ErrorCode::InvalidInput);

    let mut tap_with_text =
        ActivationObservation::tap(id("actionable"), receipt.schedule_epoch, now);
    tap_with_text.text_input = Some(String::from("hello"));
    assert_eq!(
        fixture
            .service
            .record_activation(&OperationContext::default(), tap_with_text)
            .expect_err("body tap with text")
            .code,
        ErrorCode::InvalidInput
    );

    let mut control = ActivationObservation::tap(id("actionable"), receipt.schedule_epoch, now);
    control.text_input = Some(String::from("bad\ninput"));
    assert_eq!(
        fixture
            .service
            .record_activation(&OperationContext::default(), control)
            .expect_err("control text")
            .code,
        ErrorCode::InvalidInput
    );

    let text_receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("reply"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                text_input_content(),
            ),
        )
        .expect("submitted reply");
    let action_id = ActionId::new(SCHEDULE_TEXT_INPUT_ACTION_ID).expect("reserved");
    let mut reply =
        ActivationObservation::action(id("reply"), text_receipt.schedule_epoch, action_id, now);
    reply.text_input = Some(String::from("On my way"));
    let handoff = fixture
        .service
        .record_activation(&OperationContext::default(), reply)
        .expect("text input");
    assert_eq!(handoff.text_input.as_deref(), Some("On my way"));

    let mut too_long = ActivationObservation::action(
        id("reply"),
        text_receipt.schedule_epoch,
        ActionId::new(SCHEDULE_TEXT_INPUT_ACTION_ID).expect("reserved"),
        now,
    );
    too_long.text_input = Some("a".repeat(MAX_TEXT_INPUT_BYTES + 1));
    assert!(
        fixture
            .service
            .record_activation(&OperationContext::default(), too_long)
            .is_err()
    );
}

#[test]
fn stale_epoch_callbacks_fail_closed_after_cancel_and_replace() {
    let directory = TestDirectory::new("evidence-stale");
    let fixture = Fixture::open(directory.path());
    let first = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stale"),
                ScheduleTrigger::RepeatingInterval {
                    interval_ms: MIN_REPEATING_INTERVAL_MS,
                    max_occurrences: None,
                },
                content("Stale"),
            ),
        )
        .expect("first");
    let old_epoch = first.schedule_epoch;
    let cancelled = fixture
        .service
        .cancel(&OperationContext::default(), &id("stale"))
        .expect("cancel");
    assert!(!cancelled.already_terminal);
    assert!(cancelled.schedule_epoch > old_epoch);
    assert_eq!(
        fixture
            .service
            .record_delivery_observation(
                &OperationContext::default(),
                DeliveryObservation::new(id("stale"), old_epoch, first.submitted_at_epoch_ms + 10),
            )
            .expect_err("stale presentation")
            .code,
        ErrorCode::ContextExpired
    );
    assert_eq!(
        fixture
            .service
            .record_activation(
                &OperationContext::default(),
                ActivationObservation::tap(
                    id("stale"),
                    old_epoch,
                    first.submitted_at_epoch_ms + 10
                ),
            )
            .expect_err("stale activation")
            .code,
        ErrorCode::ContextExpired
    );

    let replacement = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stale"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Replacement"),
            )
            .with_conflict_policy(ScheduleConflictPolicy::Replace),
        )
        .expect("replace");
    assert!(replacement.schedule_epoch > cancelled.schedule_epoch);
    assert_eq!(
        fixture
            .service
            .record_delivery_observation(
                &OperationContext::default(),
                DeliveryObservation::new(
                    id("stale"),
                    first.schedule_epoch,
                    replacement.submitted_at_epoch_ms + 10,
                ),
            )
            .expect_err("stale replaced presentation")
            .code,
        ErrorCode::ContextExpired
    );
}

#[test]
fn exact_alarm_requires_capability_and_special_access_or_downgrades() {
    let directory = TestDirectory::new("evidence-exact");
    let fixture = Fixture::open(directory.path());
    let submission = |name: &str, fallback| {
        ScheduleSubmission::new(
            id(name),
            ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
            content(name),
        )
        .with_precision(SchedulePrecision::ExactAlarm, fallback)
    };

    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Unsupported);
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            submission("downgrade-capability", ExactAlarmFallback::AllowInexact),
        )
        .expect("downgraded");
    assert_eq!(receipt.precision, SchedulePrecision::InexactAdvisory);
    assert_eq!(
        receipt.disposition,
        SubmissionDisposition::DowngradedToInexact
    );
    assert_eq!(
        receipt.downgrade.expect("downgrade").reason,
        InexactDowngradeReason::ExactAlarmCapabilityUnavailable
    );

    assert_eq!(
        fixture
            .service
            .submit(
                &OperationContext::default(),
                submission("reject-capability", ExactAlarmFallback::Reject),
            )
            .expect_err("reject")
            .code,
        ErrorCode::Unsupported
    );

    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Native);
    fixture
        .permissions
        .set_exact_alarm(PermissionStatus::Denied {
            can_request_again: true,
        });
    let downgraded = fixture
        .service
        .submit(
            &OperationContext::default(),
            submission("downgrade-permission", ExactAlarmFallback::AllowInexact),
        )
        .expect("permission downgrade");
    assert_eq!(
        downgraded.downgrade.expect("downgrade").reason,
        InexactDowngradeReason::ExactAlarmPermissionDenied
    );
    assert_eq!(
        fixture
            .service
            .submit(
                &OperationContext::default(),
                submission("reject-permission", ExactAlarmFallback::Reject),
            )
            .expect_err("permission reject")
            .code,
        ErrorCode::PermissionDenied
    );

    fixture
        .permissions
        .set_exact_alarm(PermissionStatus::Granted);
    let exact = fixture
        .service
        .submit(
            &OperationContext::default(),
            submission("exact", ExactAlarmFallback::Reject),
        )
        .expect("exact");
    assert_eq!(exact.precision, SchedulePrecision::ExactAlarm);
    assert!(exact.downgrade.is_none());
}

#[test]
fn downgraded_submission_emits_submitted_and_downgrade_events() {
    let directory = TestDirectory::new("evidence-downgrade-event");
    let fixture = Fixture::open(directory.path());
    let subscription = fixture.service.subscribe();
    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Unsupported);
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("downgrade-event"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Downgrade event"),
            )
            .with_precision(
                SchedulePrecision::ExactAlarm,
                ExactAlarmFallback::AllowInexact,
            ),
        )
        .expect("downgraded submission");
    assert_eq!(receipt.precision, SchedulePrecision::InexactAdvisory);
    let events = drain_events(&subscription);
    assert!(matches!(
        events.as_slice(),
        [
            NotificationScheduleEvent::Submitted {
                precision: SchedulePrecision::InexactAdvisory,
                ..
            },
            NotificationScheduleEvent::DowngradedToInexact {
                reason: InexactDowngradeReason::ExactAlarmCapabilityUnavailable,
                ..
            }
        ]
    ));
}

#[test]
fn repeat_and_attachment_capabilities_and_attachment_validation_gate_submission() {
    let directory = TestDirectory::new("evidence-capabilities");
    let fixture = Fixture::open(directory.path());

    fixture.provider.set_support(
        ScheduleOperation::RepeatAndCalendarSchedule,
        Support::Unsupported,
    );
    assert_eq!(
        fixture
            .service
            .submit(
                &OperationContext::default(),
                ScheduleSubmission::new(
                    id("repeat"),
                    ScheduleTrigger::RepeatingInterval {
                        interval_ms: MIN_REPEATING_INTERVAL_MS,
                        max_occurrences: None,
                    },
                    content("Repeat"),
                ),
            )
            .expect_err("repeat gate")
            .code,
        ErrorCode::Unsupported
    );

    fixture.provider.set_support(
        ScheduleOperation::RepeatAndCalendarSchedule,
        Support::Native,
    );
    fixture.provider.set_support(
        ScheduleOperation::AttachMediaToSchedule,
        Support::Unsupported,
    );
    let valid_attachment = ScheduleAttachment::new(
        AttachmentSource::BundledAsset(String::from("assets/icon.png")),
        "image/png",
        1_024,
    )
    .expect("attachment");
    assert_eq!(
        fixture
            .service
            .submit(
                &OperationContext::default(),
                ScheduleSubmission::new(
                    id("attach"),
                    ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                    content("Attach"),
                )
                .with_attachments(vec![valid_attachment.clone()]),
            )
            .expect_err("attachment gate")
            .code,
        ErrorCode::Unsupported
    );

    fixture
        .provider
        .set_support(ScheduleOperation::AttachMediaToSchedule, Support::Native);
    let remote = ScheduleAttachment::new(
        AttachmentSource::PrivateFile(String::from("https://example.com/a.png")),
        "image/png",
        10,
    );
    assert!(remote.is_err());
    let traversal = ScheduleAttachment::new(
        AttachmentSource::PrivateFile(String::from("../a.png")),
        "image/png",
        10,
    );
    assert!(traversal.is_err());
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("attach-ok"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Attach ok"),
            )
            .with_attachments(vec![valid_attachment]),
        )
        .expect("accepted attachment");
    assert!(receipt.is_scheduled());
}

#[test]
fn conflict_policies_and_active_budget_are_enforced() {
    let directory = TestDirectory::new("evidence-conflict");
    let retention = RetentionPolicy {
        max_active_schedules: 1,
        ..RetentionPolicy::default()
    };
    let fixture = Fixture::open_with_policy(directory.path(), retention);
    let first = submit_after_delay(&fixture, "one", 60_000);
    let second = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("one"),
                ScheduleTrigger::AfterDelay { delay_ms: 90_000 },
                content("One kept"),
            )
            .with_conflict_policy(ScheduleConflictPolicy::KeepExisting),
        )
        .expect("kept existing");
    assert_eq!(second.disposition, SubmissionDisposition::KeptExisting);
    assert_eq!(second.schedule_epoch, first.schedule_epoch);
    assert_eq!(fixture.provider.request_count(), 1);

    assert_eq!(
        fixture
            .service
            .submit(
                &OperationContext::default(),
                ScheduleSubmission::new(
                    id("one"),
                    ScheduleTrigger::AfterDelay { delay_ms: 90_000 },
                    content("One conflict"),
                )
                .with_conflict_policy(ScheduleConflictPolicy::RejectConflict),
            )
            .expect_err("conflict")
            .code,
        ErrorCode::InvalidInput
    );

    let replaced = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("one"),
                ScheduleTrigger::AfterDelay { delay_ms: 120_000 },
                content("One replaced"),
            )
            .with_conflict_policy(ScheduleConflictPolicy::Replace),
        )
        .expect("replaced");
    assert_eq!(replaced.disposition, SubmissionDisposition::Replaced);
    assert!(replaced.schedule_epoch > first.schedule_epoch);

    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("two"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Two"),
            ),
        )
        .expect_err("active budget");
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(
        fixture
            .service
            .active_schedules()
            .iter()
            .filter(|record| record.is_pending())
            .count(),
        1
    );
}

#[test]
fn activation_handoff_carries_the_bound_route_and_action() {
    let directory = TestDirectory::new("evidence-handoff");
    let fixture = Fixture::open(directory.path());
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("handoff"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                actionable_content(),
            ),
        )
        .expect("submitted");
    let action = ActionId::new("snooze").expect("action");
    let handoff = fixture
        .service
        .record_activation(
            &OperationContext::default(),
            ActivationObservation::action(
                id("handoff"),
                receipt.schedule_epoch,
                action.clone(),
                receipt.submitted_at_epoch_ms + 1_000,
            ),
        )
        .expect("activation");
    assert_eq!(handoff.action_id.as_ref(), Some(&action));
    assert_eq!(handoff.action_label(&actionable_content()), Some("Snooze"));
    let target = handoff.target.as_ref().expect("target");
    assert_eq!(target.route(), "schedule");
    assert_eq!(
        target.parameters().get("schedule").map(String::as_str),
        Some("reminder")
    );
    let payload = handoff.payload.as_ref().expect("payload");
    match payload {
        ActivationPayload::Notification {
            notification_id,
            target,
        } => {
            assert_eq!(notification_id, &handoff.notification_activation_id);
            assert_eq!(target.route(), "schedule");
        }
        other => panic!("unexpected payload {other:?}"),
    }
    let event = handoff
        .into_activation_event(std::time::SystemTime::now())
        .expect("activation event");
    assert_eq!(event.source(), ActivationSource::Notification);
}

#[test]
fn ui_renderer_and_cancelled_owners_are_rejected_before_side_effects() {
    let directory = TestDirectory::new("evidence-owner");
    let fixture = Fixture::open(directory.path());
    let owner = UiOwner::new(SurfaceId(9));
    let ui_context = OperationContext {
        ui: Some(owner.context(None).expect("ui context")),
        ..OperationContext::default()
    };
    let submission = ScheduleSubmission::new(
        id("owner"),
        ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
        content("Owner"),
    );
    assert_eq!(
        fixture
            .service
            .submit(&ui_context, submission.clone())
            .expect_err("ui owner")
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        ExecutionOwner::Renderer
            .require_background(&OperationContext::default())
            .expect_err("renderer owner")
            .code,
        ErrorCode::InvalidInput
    );
    assert!(
        ExecutionOwner::Ui
            .require_background(&OperationContext::default())
            .is_err()
    );
    assert!(
        ExecutionOwner::Background
            .require_background(&OperationContext::default())
            .is_ok()
    );

    let cancellation = gpui_notification_schedule::CancellationToken::default();
    cancellation.cancel(CancelReason::Caller);
    let cancelled = OperationContext {
        cancellation,
        ..OperationContext::default()
    };
    assert_eq!(
        fixture
            .service
            .submit(&cancelled, submission)
            .expect_err("cancelled")
            .code,
        ErrorCode::ContextExpired
    );
    assert_eq!(fixture.provider.request_count(), 0);
    assert_eq!(fixture.service.schedule_count(), 0);
    assert!(
        ensure_not_cancelled(&gpui_notification_schedule::CancellationToken::default()).is_ok()
    );
}

#[test]
fn events_are_ordered_and_keep_the_three_stages_separate() {
    let directory = TestDirectory::new("evidence-events");
    let fixture = Fixture::open(directory.path());
    let subscription = fixture.service.subscribe();
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stages"),
                ScheduleTrigger::RepeatingInterval {
                    interval_ms: MIN_REPEATING_INTERVAL_MS,
                    max_occurrences: None,
                },
                content("Stages"),
            ),
        )
        .expect("submitted");
    let fire = receipt.next_fire_at_epoch_ms.expect("fire");
    fixture
        .service
        .record_delivery_observation(
            &OperationContext::default(),
            DeliveryObservation::new(id("stages"), receipt.schedule_epoch, fire),
        )
        .expect("presented");
    fixture
        .service
        .record_activation(
            &OperationContext::default(),
            ActivationObservation::tap(id("stages"), receipt.schedule_epoch, fire + 1),
        )
        .expect("activated");
    let events = drain_events(&subscription);
    assert!(matches!(
        events.as_slice(),
        [
            NotificationScheduleEvent::Submitted { .. },
            NotificationScheduleEvent::Presented { .. },
            NotificationScheduleEvent::Activated { .. }
        ]
    ));
    let mut map = BTreeMap::new();
    for event in events {
        map.insert(event.schedule_id().expect("schedule id").clone(), event);
    }
    assert_eq!(map.len(), 1);
}
