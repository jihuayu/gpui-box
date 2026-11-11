//! Shared deterministic fixtures for notification-schedule integration tests.

#![allow(dead_code)]

use gpui_notification_schedule::*;
use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "gpui-notification-schedule-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("test directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub struct Fixture {
    pub service: NotificationScheduleService,
    pub app: AppData,
    pub provider: Arc<FakeNotificationScheduleProvider>,
    pub clock: Arc<FakeScheduleClock>,
    pub permissions: Arc<ScheduleAuthorization>,
}

impl Fixture {
    pub fn open(path: impl AsRef<Path>) -> Self {
        Self::open_with_policy(path, RetentionPolicy::default())
    }

    pub fn open_with_policy(path: impl AsRef<Path>, retention: RetentionPolicy) -> Self {
        Self::open_with_policy_and_hook(path, retention, Arc::new(NoopScheduleStorageHook))
    }

    pub fn open_with_hook(path: impl AsRef<Path>, hook: Arc<dyn ScheduleStorageHook>) -> Self {
        Self::open_with_policy_and_hook(path, RetentionPolicy::default(), hook)
    }

    pub fn open_with_policy_and_hook(
        path: impl AsRef<Path>,
        retention: RetentionPolicy,
        hook: Arc<dyn ScheduleStorageHook>,
    ) -> Self {
        let app = AppData::open(DirectoryRoots::under(path)).expect("private app data");
        let provider = Arc::new(FakeNotificationScheduleProvider::new());
        let clock = Arc::new(FakeScheduleClock::new(1_700_000_000_000));
        let permissions = Arc::new(ScheduleAuthorization::granting());
        let service = NotificationScheduleService::open_with_options(
            &app,
            provider.clone(),
            clock.clone(),
            Arc::new(BuiltinTimeZoneResolver::new()),
            permissions.clone(),
            retention,
            hook,
        )
        .expect("durable notification schedule service");
        Self {
            service,
            app,
            provider,
            clock,
            permissions,
        }
    }
}

pub fn id(name: &str) -> ScheduledNotificationId {
    ScheduledNotificationId::new(ScheduleNamespace::application(), name).expect("schedule id")
}

pub fn content(title: &str) -> NotificationContent {
    NotificationContent::new(title, "Body").expect("notification content")
}

pub fn channelled_content(title: &str, channel: &str) -> NotificationContent {
    content(title)
        .with_channel_id(channel)
        .expect("notification channel")
}

pub fn actionable_content() -> NotificationContent {
    let action_id = ActionId::new("snooze").expect("action id");
    let target = ActivationTarget::new(
        "schedule",
        1,
        BTreeMap::from([("schedule".to_owned(), "reminder".to_owned())]),
    )
    .expect("activation target");
    NotificationContent::new("Reminder", "Stand up")
        .expect("content")
        .with_actions(vec![
            NotificationAction::new(action_id.clone(), "Snooze").expect("action"),
        ])
        .expect("actions")
        .with_activation(
            NotificationActivationBinding::new(target.clone())
                .bind(action_id, target)
                .expect("binding"),
        )
}

pub fn text_input_content() -> NotificationContent {
    let action_id = ActionId::new(SCHEDULE_TEXT_INPUT_ACTION_ID).expect("reserved action id");
    NotificationContent::new("Reminder", "Reply to the reminder")
        .expect("content")
        .with_actions(vec![
            NotificationAction::new(action_id, "Reply").expect("action"),
        ])
        .expect("actions")
}

pub fn at_utc(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> u64 {
    epoch_ms_from_civil(year, month, day, hour, minute, 0).expect("civil instant")
}

pub fn submit_after_delay(
    fixture: &Fixture,
    name: &str,
    delay_ms: u64,
) -> ScheduleSubmissionReceipt {
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id(name),
                ScheduleTrigger::AfterDelay { delay_ms },
                content(name),
            ),
        )
        .expect("submitted")
}

pub fn drain_events(
    subscription: &EventSubscription<NotificationScheduleEvent>,
) -> Vec<NotificationScheduleEvent> {
    let mut events = Vec::new();
    while let Ok(mut next) = subscription.try_next() {
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        match std::pin::Pin::new(&mut next).poll(&mut context) {
            std::task::Poll::Ready(Some(envelope)) => match envelope.payload {
                Event::Value(value) => events.push(value),
                Event::ResyncRequired => {}
            },
            std::task::Poll::Ready(None) | std::task::Poll::Pending => break,
        }
    }
    events
}
