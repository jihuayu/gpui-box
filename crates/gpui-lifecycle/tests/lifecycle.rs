use gpui_lifecycle::*;
use gpui_system::{DiagnosticPhase, ErrorCode, SystemError, SystemServices};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant, SystemTime},
};

fn sample_snapshot(schema: SchemaVersion) -> VersionedSnapshot {
    let mut route = RouteIdentity::new("editor");
    let params = &mut route.params;
    params.insert("document".to_owned(), "draft".to_owned());
    let mut snapshot = VersionedSnapshot::new(schema, 1, route);
    snapshot.focus = Some(FocusTarget::new("title"));
    let draft = FieldDraft::new("title")
        .with_text("Saved", "unsaved")
        .with_selection(SelectionRange::new(3, 7), SelectionRange::new(3, 7))
        .with_composition(true);
    snapshot.drafts.insert("title".to_string(), draft);
    snapshot
}

fn stale(found: u64, current: u64) -> RestoreOutcome {
    RestoreOutcome::StaleGeneration { found, current }
}

fn mismatch(found: u32, expected: u32) -> RestoreOutcome {
    RestoreOutcome::SchemaMismatch {
        found: SchemaVersion(found),
        expected: SchemaVersion(expected),
    }
}

#[derive(Debug, Default)]
struct ByteStore {
    entries: BTreeMap<String, Vec<u8>>,
}

impl ByteStore {
    fn insert_raw(&mut self, key: &str, payload: impl Into<Vec<u8>>) {
        self.entries.insert(key.to_string(), payload.into());
    }
}

impl SnapshotStore for ByteStore {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, SystemError> {
        Ok(self.entries.get(key).cloned())
    }

    fn save(&mut self, key: &str, payload: &[u8]) -> Result<(), SystemError> {
        self.entries.insert(key.to_string(), payload.to_vec());
        Ok(())
    }
}

fn chained_outcome(
    hops: &[(SchemaVersion, SchemaVersion)],
    expected: SchemaVersion,
) -> RestoreOutcome {
    let mut store = ByteStore::default();
    let initial = SchemaVersion(1);
    store.insert_raw("session", sample_snapshot(initial).encode());
    let mut coordinator = RestorationCoordinator::new(expected, store);
    for (from, to) in hops {
        let from = *from;
        let to = *to;
        coordinator.register_migration(SchemaMigration::new(from, to, move |_, payload| {
            let mut snapshot = VersionedSnapshot::decode(payload)?;
            snapshot.schema = to;
            Ok(snapshot.encode())
        }));
    }
    coordinator.restore("session", 0)
}

fn poll_next<T>(
    subscription: &EventSubscription<T>,
    waker: &Waker,
) -> Poll<Option<EventEnvelope<T>>> {
    let mut next = subscription.try_next().expect("single reader");
    let mut context = Context::from_waker(waker);
    std::future::Future::poll(Pin::new(&mut next), &mut context)
}

fn contains_lifecycle(events: &[EventEnvelope<LifecycleEvent>], expected: &LifecycleEvent) -> bool {
    events
        .iter()
        .any(|event| event.payload == Event::Value(expected.clone()))
}

#[test]
fn preference_and_layout_changes_preserve_editing_state() {
    let mut route = RouteIdentity::new("editor");
    let params = &mut route.params;
    params.insert("document".to_owned(), "draft-1".to_owned());
    let metrics = ViewportMetrics::new(1024.0, 768.0, 2.0);
    let mut scene = SceneState::new(route.clone(), metrics);
    let focused = FocusTarget::new("field:title");
    scene.set_focus(Some(focused.clone()));
    let draft = FieldDraft::new("title")
        .with_text("Saved", "Unsaved text")
        .with_selection(SelectionRange::new(7, 3), SelectionRange::new(9, 5))
        .with_composition(true);
    scene.set_draft(draft.clone());
    assert_eq!(scene.layout.size_class, SizeClass::Expanded);

    let dark = SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        ..SystemPreferences::default()
    };
    assert!(!scene.apply_preferences(dark.clone()));
    assert_eq!(scene.route, route);
    assert_eq!(scene.focus.as_ref(), Some(&focused));
    assert_eq!(scene.draft("title"), Some(&draft));

    let large_text = SystemPreferences {
        text_scale: PreferenceValue::Known(TextScale::new(2.0).unwrap()),
        ..dark
    };
    assert!(scene.apply_preferences(large_text));
    assert_eq!(scene.layout.size_class, SizeClass::Compact);
    assert_eq!(scene.route, route);
    assert_eq!(scene.focus.as_ref(), Some(&focused));

    let current = scene.draft("title").unwrap();
    assert_eq!(current.draft, "Unsaved text");
    assert_eq!(current.selection_utf16, SelectionRange::new(7, 3));
    assert_eq!(current.selection_bytes, SelectionRange::new(9, 5));
    assert!(current.composing);

    let keyboard = EdgeInsets {
        top: 0.0,
        right: 0.0,
        bottom: 400.0,
        left: 0.0,
    };
    let metrics = ViewportMetrics::new(1024.0, 768.0, 2.0).with_insets(EdgeInsets::ZERO, keyboard);
    assert!(!scene.apply_viewport(metrics));
    assert_eq!(scene.layout.metrics.keyboard.bottom, 400.0);
    assert_eq!(scene.layout.size_class, SizeClass::Compact);

    let wide = ViewportMetrics::new(2048.0, 1200.0, 2.0);
    assert!(scene.apply_viewport(wide));
    assert_eq!(scene.layout.size_class, SizeClass::Expanded);
    assert_eq!(scene.route, route);
    assert_eq!(scene.draft("title"), Some(&draft));
}

#[test]
fn host_recreation_rejects_stale_callbacks_and_preserves_state() {
    let mut host = FakeHost::new(ProcessId(7));
    let surface = SurfaceId(3);
    assert!(host.register(surface, 1));
    assert!(host.show(surface, 1, true));
    let now = host.now();
    host.service()
        .set_phase_at(AppPhase::ForegroundActive, now)
        .unwrap();
    assert_eq!(host.service().phase(), AppPhase::ForegroundActive);
    host.advance(Duration::from_secs(5));
    assert_eq!(host.active_duration(), Duration::from_secs(5));

    let generation = host.rotate(surface);
    assert_eq!(generation, 2);
    assert_eq!(host.active_duration(), Duration::from_secs(5));

    assert!(!host.hide(surface, 1));
    let rejections = host.service().rejections();
    let rejected = rejections.last().unwrap();
    assert_eq!(rejected.received_generation, 1);
    assert_eq!(rejected.current_generation, Some(2));
    assert_eq!(rejected.phase, DiagnosticPhase::LateCallbackDiscarded);

    host.background();
    assert_eq!(host.service().phase(), AppPhase::Background);
    assert_eq!(host.active_duration(), Duration::from_secs(5));
    host.advance(Duration::from_secs(10));
    host.resume();
    assert_eq!(host.service().phase(), AppPhase::ForegroundActive);

    host.lock();
    assert_eq!(host.service().session_lock(), SessionLockState::Locked);
    host.unlock();
    assert_eq!(host.service().session_lock(), SessionLockState::Unlocked);

    host.service()
        .set_memory_pressure(MemoryPressure::Warning)
        .unwrap();
    assert_eq!(host.service().memory_pressure(), MemoryPressure::Warning);

    let now = host.now();
    let stale_snapshot = SurfaceSnapshot {
        visibility: SurfaceVisibility::Hidden,
        generation: 1,
    };
    let applied = host
        .service()
        .apply_surface_at(surface, stale_snapshot, now)
        .unwrap();
    assert!(!applied);
    let detached = host
        .service()
        .detach_surface_at(surface, generation, now)
        .unwrap();
    assert!(detached);
    assert!(host.service().surface(surface).is_none());
    let visible = SurfaceVisibility::Visible { focused: true };
    let revived = SurfaceSnapshot {
        visibility: visible,
        generation,
    };
    let applied = host
        .service()
        .apply_surface_at(surface, revived, now)
        .unwrap();
    assert!(!applied);

    let schema = SchemaVersion(2);
    let mut coordinator = RestorationCoordinator::new(schema, InMemorySnapshotStore::new());
    let saved = sample_snapshot(schema);
    assert!(coordinator.save("session", &saved).is_ok());
    coordinator.set_host_generation(6);
    let metrics = ViewportMetrics::new(400.0, 800.0, 1.0);
    let mut scene = SceneState::new(RouteIdentity::new("home"), metrics);
    let stale_request = coordinator.restore_into("session", 5, &mut scene);
    assert_eq!(stale_request, stale(5, 6));
    assert_eq!(scene.route, RouteIdentity::new("home"));
    assert!(scene.drafts().is_empty());

    let restored = coordinator.restore_into("session", 6, &mut scene);
    assert!(matches!(restored, RestoreOutcome::Restored(_)));
    assert_eq!(scene.route.param("document"), Some("draft"));
}

#[test]
fn overlapping_windows_record_one_monotonic_union() {
    let mut tracker = ForegroundUseTracker::new(ProcessId(1), SessionId(9));
    let origin = Instant::now();
    let window_a = SurfaceId(10);
    let window_b = SurfaceId(11);
    assert!(tracker.set_phase(AppPhase::ForegroundActive, origin));
    assert!(tracker.set_surface_visible(window_a, true, origin));
    let second = origin + Duration::from_secs(5);
    assert!(tracker.set_surface_visible(window_b, true, second));
    let first_hide = origin + Duration::from_secs(10);
    assert!(tracker.set_surface_visible(window_a, false, first_hide));

    let live = origin + Duration::from_secs(12);
    let live_total = tracker.total_active_duration(live);
    assert_eq!(live_total, Duration::from_secs(12));

    let closed = origin + Duration::from_secs(15);
    assert!(tracker.set_surface_visible(window_b, false, closed));
    let total = tracker.total_active_duration(closed);
    assert_eq!(total, Duration::from_secs(15));
    assert_eq!(tracker.visibility().intervals().len(), 1);
    let interval = tracker.visibility().intervals()[0];
    assert_eq!(interval.duration(), Duration::from_secs(15));

    let wall = SystemTime::now();
    tracker.record_wall_clock_observation(wall);
    let before = tracker.total_active_duration(closed);
    let backwards = wall - Duration::from_secs(86_400);
    tracker.record_wall_clock_observation(backwards);
    let shifted = wall + Duration::from_secs(86_400);
    tracker.record_wall_clock_observation(shifted);
    let after = tracker.total_active_duration(closed);
    assert_eq!(before, after);
    assert_eq!(after, Duration::from_secs(15));
    assert_eq!(tracker.last_wall_clock(), Some(shifted));

    let input = origin + Duration::from_secs(3);
    assert!(tracker.record_input_at(input));
    assert!(!tracker.record_input_at(origin));
    assert_eq!(tracker.last_input_at(), Some(input));
    assert_eq!(tracker.idle_state(), IdleState::Unavailable);
    let since = origin + Duration::from_secs(6);
    tracker.set_idle_state(IdleState::Idle { since });
    let idle = tracker.idle_state();
    assert_eq!(idle.idle_since(), Some(since));
    let idle_for = origin + Duration::from_secs(8);
    let elapsed = tracker.elapsed_since_input(idle_for);
    assert_eq!(elapsed, Some(Duration::from_secs(5)));

    let mut live_tracker = ForegroundUseTracker::new(ProcessId(2), SessionId(9));
    assert!(live_tracker.set_phase(AppPhase::ForegroundActive, origin));
    assert!(live_tracker.set_surface_visible(window_a, true, origin));
    let live_for = origin + Duration::from_secs(7);
    let partial = live_tracker.total_active_duration(live_for);
    assert_eq!(partial, Duration::from_secs(7));
}

#[test]
fn foreground_intervals_require_a_foreground_phase() {
    let mut tracker = ForegroundUseTracker::new(ProcessId(1), SessionId(2));
    let origin = Instant::now();
    let window_a = SurfaceId(1);
    let window_b = SurfaceId(2);
    assert!(tracker.set_surface_visible(window_a, true, origin));
    let still_launching = origin + Duration::from_secs(10);
    assert_eq!(
        tracker.total_active_duration(still_launching),
        Duration::ZERO
    );

    assert!(tracker.set_phase(AppPhase::ForegroundActive, still_launching));
    let active = origin + Duration::from_secs(20);
    assert_eq!(
        tracker.total_active_duration(active),
        Duration::from_secs(10)
    );

    assert!(tracker.set_phase(AppPhase::Background, active));
    let background = origin + Duration::from_secs(25);
    assert!(tracker.set_surface_visible(window_b, true, background));
    assert!(tracker.is_surface_visible(window_a));
    assert!(tracker.is_surface_visible(window_b));
    let paused = origin + Duration::from_secs(50);
    assert_eq!(
        tracker.total_active_duration(paused),
        Duration::from_secs(10)
    );

    assert!(tracker.set_phase(AppPhase::ForegroundActive, paused));
    let resumed = origin + Duration::from_secs(60);
    assert_eq!(
        tracker.total_active_duration(resumed),
        Duration::from_secs(20)
    );

    let one_hidden = origin + Duration::from_secs(55);
    assert!(tracker.set_surface_visible(window_a, false, one_hidden));
    assert_eq!(
        tracker.total_active_duration(resumed),
        Duration::from_secs(20)
    );

    assert!(tracker.set_phase(AppPhase::Suspended, resumed));
    let suspended = origin + Duration::from_secs(80);
    assert_eq!(
        tracker.total_active_duration(suspended),
        Duration::from_secs(20)
    );
    assert!(tracker.is_surface_visible(window_b));
}

#[test]
fn service_foreground_guard_retains_visible_surfaces() {
    let mut host = FakeHost::new(ProcessId(3));
    let surface = SurfaceId(5);
    assert!(host.register(surface, 1));
    assert!(host.show(surface, 1, true));
    host.advance(Duration::from_secs(10));
    assert_eq!(host.active_duration(), Duration::ZERO);

    let now = host.now();
    host.service()
        .set_phase_at(AppPhase::ForegroundActive, now)
        .unwrap();
    host.advance(Duration::from_secs(10));
    assert_eq!(host.active_duration(), Duration::from_secs(10));

    host.background();
    assert!(
        host.service()
            .surface(surface)
            .unwrap()
            .visibility
            .is_visible()
    );
    host.advance(Duration::from_secs(10));
    assert_eq!(host.active_duration(), Duration::from_secs(10));

    host.resume();
    host.advance(Duration::from_secs(5));
    assert_eq!(host.active_duration(), Duration::from_secs(15));
}

#[test]
fn restoration_reports_recoverable_failures() {
    let schema = SchemaVersion(2);
    let snapshot = sample_snapshot(schema);
    let failing = InMemorySnapshotStore::failing_saves();
    let mut coordinator = RestorationCoordinator::new(schema, failing);
    assert!(coordinator.save("session", &snapshot).is_err());
    let outcome = coordinator.last_outcome().cloned();
    assert_eq!(outcome, Some(RestoreOutcome::SaveFailed));

    let mut corrupt = InMemorySnapshotStore::new();
    corrupt.insert_raw("session", b"not-a-snapshot".to_vec());
    let mut coordinator = RestorationCoordinator::new(schema, corrupt);
    let outcome = coordinator.restore("session", 0);
    assert_eq!(outcome, RestoreOutcome::CorruptSnapshot);
    let outcome = coordinator.restore("missing", 0);
    assert_eq!(outcome, RestoreOutcome::AbsentSnapshot);

    let legacy = sample_snapshot(SchemaVersion(1));
    let mut old = InMemorySnapshotStore::new();
    old.insert_raw("session", legacy.encode());
    let mut coordinator = RestorationCoordinator::new(schema, old);
    let outcome = coordinator.restore("session", 0);
    assert_eq!(outcome, mismatch(1, 2));

    let mut old = InMemorySnapshotStore::new();
    old.insert_raw("session", legacy.encode());
    let mut coordinator = RestorationCoordinator::new(schema, old);
    let migrate = move |from: SchemaVersion, payload: &[u8]| -> Result<Vec<u8>, SystemError> {
        let mut snapshot = VersionedSnapshot::decode(payload)?;
        assert_eq!(from, SchemaVersion(1));
        snapshot.schema = schema;
        Ok(snapshot.encode())
    };
    let migration = SchemaMigration::new(SchemaVersion(1), SchemaVersion(2), migrate);
    coordinator.register_migration(migration);
    let outcome = coordinator.restore("session", 0);
    match outcome {
        RestoreOutcome::Migrated { from, snapshot } => {
            assert_eq!(from, SchemaVersion(1));
            assert_eq!(snapshot.schema, schema);
            assert_eq!(snapshot.drafts["title"].draft, "unsaved");
        }
        other => panic!("expected a migration, got {other:?}"),
    }

    let mut old = InMemorySnapshotStore::new();
    old.insert_raw("session", legacy.encode());
    let mut coordinator = RestorationCoordinator::new(schema, old);
    let migrate = |_: SchemaVersion, _: &[u8]| -> Result<Vec<u8>, SystemError> {
        Err(SystemError::new(ErrorCode::InvalidInput, "cannot migrate"))
    };
    let migration = SchemaMigration::new(SchemaVersion(1), SchemaVersion(2), migrate);
    coordinator.register_migration(migration);
    let outcome = coordinator.restore("session", 0);
    assert_eq!(outcome, mismatch(1, 2));

    let preferences = SystemPreferences::default();
    assert!(!preferences.appearance.is_available());
    assert!(!preferences.text_scale.is_available());
    assert!(!preferences.high_contrast.is_available());
    assert!(!preferences.reduced_motion.is_available());
    let appearance = preferences.appearance_or(Appearance::Dark);
    assert_eq!(appearance, Appearance::Dark);
    assert!(preferences.reduced_motion_or(true));

    assert!(TextScale::new(0.1).is_err());
    assert!(TextScale::new(f32::NAN).is_err());
    assert!(TextScale::new(8.5).is_err());
    assert!(TextScale::new(1.25).is_ok());
    assert!(LocaleTag::parse("zh-Hant-TW").is_ok());
    assert!(LocaleTag::parse("").is_err());
    assert!(LocaleTag::parse("en US").is_err());
    assert!(LocaleTag::parse("é").is_err());

    let metrics = ViewportMetrics::new(1024.0, 768.0, 1.0);
    let mut scene = SceneState::new(RouteIdentity::new("home"), metrics);
    assert_eq!(scene.text_scale(), TextScale::default());
    assert!(!scene.apply_preferences(preferences));
    assert_eq!(scene.text_scale(), TextScale::default());
}

#[test]
fn multi_hop_migration_chains_and_rejects_bad_hops() {
    let v1 = SchemaVersion(1);
    let v2 = SchemaVersion(2);
    let v3 = SchemaVersion(3);
    let migrated = chained_outcome(&[(v1, v2), (v2, v3)], v3);
    assert!(matches!(
        migrated,
        RestoreOutcome::Migrated { from, ref snapshot }
            if from == v1 && snapshot.schema == v3
    ));

    assert_eq!(chained_outcome(&[(v1, v2)], v3), mismatch(1, 3));
    assert_eq!(chained_outcome(&[(v1, v2), (v2, v1)], v3), mismatch(1, 3));
    assert_eq!(chained_outcome(&[(v1, v3)], v2), mismatch(1, 2));

    let mut store = ByteStore::default();
    store.insert_raw("session", sample_snapshot(v1).encode());
    let mut coordinator = RestorationCoordinator::new(v3, store);
    coordinator.register_migration(SchemaMigration::new(v1, v2, move |_, payload| {
        let mut snapshot = VersionedSnapshot::decode(payload)?;
        snapshot.schema = v3;
        Ok(snapshot.encode())
    }));
    assert_eq!(coordinator.restore("session", 0), mismatch(1, 3));
}

#[test]
fn external_byte_store_round_trips_snapshots() {
    let schema = SchemaVersion(2);
    let mut coordinator = RestorationCoordinator::new(schema, ByteStore::default());
    let snapshot = sample_snapshot(schema);
    assert!(coordinator.save("session", &snapshot).is_ok());
    assert!(coordinator.store().entries.contains_key("session"));
    let bytes = coordinator.store().entries.get("session").unwrap();
    assert_eq!(VersionedSnapshot::peek_schema(bytes).unwrap(), schema);
    let outcome = coordinator.restore("session", 0);
    assert!(matches!(outcome, RestoreOutcome::Restored(_)));
}

#[test]
fn scoped_revalidation_cleanup_and_overflow_are_bounded() {
    let cleaned = Rc::new(Cell::new(false));
    let flag = cleaned.clone();
    let (sender, subscription) = revalidation_channel(1, move || flag.set(true));
    let subscription = subscription.with_scope(RevalidationScope::QueryClient);
    assert_eq!(subscription.scope(), RevalidationScope::QueryClient);

    let resumed = RevalidationEvent::AppResumed { generation: 1 };
    assert!(sender.publish(resumed));
    let offline = RevalidationEvent::NetworkChanged(NetworkStatus::Offline);
    assert!(sender.publish(offline.clone()));
    assert!(subscription.snapshot_required());

    let events = subscription.drain();
    assert!(matches!(&events[0].payload, Event::ResyncRequired));
    let offline_event = Event::Value(offline.clone());
    let network = events.iter().any(|event| event.payload == offline_event);
    assert!(network);

    let revision = sender.revision();
    subscription.acknowledge_snapshot(revision);
    assert!(!subscription.snapshot_required());
    drop(subscription);
    assert!(cleaned.get());
    let closed = RevalidationEvent::AppResumed { generation: 2 };
    assert!(!sender.publish(closed));

    let (_, scoped) = revalidation_channel(REVALIDATION_CAPACITY, || {});
    let scoped = scoped.with_scope(RevalidationScope::ScreenModel);
    assert_eq!(scoped.scope(), RevalidationScope::ScreenModel);
}

#[test]
fn multi_subscriber_events_survive_synchronous_waker_reentrancy() {
    thread_local! {
        static SERVICE: RefCell<Option<LifecycleService>> = const { RefCell::new(None) };
        static IN_WAKE: Cell<bool> = const { Cell::new(false) };
        static REENTERED: Cell<bool> = const { Cell::new(false) };
        static THIRD: RefCell<Option<EventSubscription<LifecycleEvent>>> =
            const { RefCell::new(None) };
    }

    struct ReentrantWake;

    impl Wake for ReentrantWake {
        fn wake(self: Arc<Self>) {
            if IN_WAKE.with(|flag| flag.replace(true)) {
                return;
            }
            let service = SERVICE.with(|slot| slot.borrow().clone());
            if let Some(service) = service {
                REENTERED.with(|flag| flag.set(true));
                let _ = service.phase();
                let _ = service.subscribe_preferences();
                let _ = service.set_session_lock(SessionLockState::Locked);
                THIRD.with(|slot| drop(slot.borrow_mut().take()));
            }
            IN_WAKE.with(|flag| flag.set(false));
        }
    }

    let service = LifecycleService::new(ProcessId(11), SessionId(12));
    let first = service.subscribe_lifecycle();
    let second = service.subscribe_lifecycle();
    let third = service.subscribe_lifecycle();
    THIRD.with(|slot| *slot.borrow_mut() = Some(third));
    let prefs_first = service.subscribe_preferences();
    let prefs_second = service.subscribe_preferences();
    let revalidation_first = service.subscribe_revalidation(RevalidationScope::QueryClient);
    let revalidation_second = service.subscribe_revalidation(RevalidationScope::ScreenModel);
    assert_eq!(service.subscriber_counts().0, 3);

    SERVICE.with(|slot| *slot.borrow_mut() = Some(service.clone()));
    let waker = Waker::from(Arc::new(ReentrantWake));
    assert!(poll_next(&first, &waker).is_pending());

    let now = Instant::now();
    assert!(
        service
            .set_phase_at(AppPhase::ForegroundActive, now)
            .unwrap()
    );
    assert!(REENTERED.with(Cell::get));
    assert_eq!(service.session_lock(), SessionLockState::Locked);
    assert_eq!(service.subscriber_counts().0, 2);

    let phase = LifecycleEvent::PhaseChanged {
        phase: AppPhase::ForegroundActive,
    };
    let lock = LifecycleEvent::SessionLockChanged {
        lock: SessionLockState::Locked,
    };
    let first_events = drain_ready(&first);
    assert_eq!(first_events.len(), 2);
    assert!(contains_lifecycle(&first_events, &phase));
    assert!(contains_lifecycle(&first_events, &lock));
    let second_events = drain_ready(&second);
    assert_eq!(second_events.len(), 2);
    assert!(contains_lifecycle(&second_events, &lock));
    assert!(contains_lifecycle(&second_events, &phase));

    let preferences = SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        ..SystemPreferences::default()
    };
    assert!(service.set_preferences(preferences).unwrap());
    assert_eq!(drain_ready(&prefs_first).len(), 1);
    assert_eq!(drain_ready(&prefs_second).len(), 1);

    assert!(service.set_network_status(NetworkStatus::Offline).unwrap());
    let offline = Event::Value(RevalidationEvent::NetworkChanged(NetworkStatus::Offline));
    assert!(
        revalidation_first
            .drain()
            .iter()
            .any(|event| event.payload == offline)
    );
    assert_eq!(revalidation_second.drain().len(), 1);

    assert!(
        service
            .set_phase_at(AppPhase::Background, now + Duration::from_secs(1))
            .unwrap()
    );
    assert!(
        service
            .set_phase_at(AppPhase::ForegroundActive, now + Duration::from_secs(2))
            .unwrap()
    );
    let resumed = revalidation_second.drain();
    assert!(resumed.iter().any(|event| matches!(
        &event.payload,
        Event::Value(RevalidationEvent::AppResumed { .. })
    )));

    drop(first);
    assert_eq!(service.subscriber_counts().0, 1);
    assert!(
        service
            .set_phase_at(AppPhase::Background, now + Duration::from_secs(3))
            .unwrap()
    );
    assert!(!drain_ready(&second).is_empty());

    SERVICE.with(|slot| *slot.borrow_mut() = None);
}

#[test]
fn lifecycle_service_registers_in_system_services() {
    let service = LifecycleService::new(ProcessId(3), SessionId(4));
    let mut services = SystemServices::new();
    service.clone().install(&mut services).unwrap();
    let registered = services.service::<LifecycleService>().unwrap();
    assert_eq!(registered.process_id(), ProcessId(3));
    assert_eq!(registered.session_id(), SessionId(4));
    registered
        .set_memory_pressure(MemoryPressure::Critical)
        .unwrap();
    assert_eq!(registered.memory_pressure(), MemoryPressure::Critical);
    assert!(registered.is_ui_thread());
    assert!(registered.require_ui_thread().is_ok());
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
}
