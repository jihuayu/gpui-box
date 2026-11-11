//! Demonstrates shared lifecycle observation, the foreground background guard,
//! multi-hop restoration and multi-subscriber revalidation.

use gpui_lifecycle::*;
use gpui_system::{SystemError, SystemServices};
use std::time::Duration;

fn main() {
    let process = ProcessId(42);
    let service = LifecycleService::new(process, SessionId(1));
    let mut services = SystemServices::new();
    service
        .clone()
        .install(&mut services)
        .expect("registers once");
    let shared = services.service::<LifecycleService>().expect("registered");
    let lifecycle_a = shared.subscribe_lifecycle();
    let lifecycle_b = shared.subscribe_lifecycle();
    let preferences = shared.subscribe_preferences();
    let revalidation_a = shared.subscribe_revalidation(RevalidationScope::QueryClient);
    let revalidation_b = shared.subscribe_revalidation(RevalidationScope::ScreenModel);

    let mut host = FakeHost::with_service(service.clone());
    let first = SurfaceId(1);
    let second = SurfaceId(2);
    assert!(host.register(first, 1));
    assert!(host.show(first, 1, true));
    assert!(host.register(second, 1));
    assert!(host.show(second, 1, false));
    host.advance(Duration::from_secs(2));
    println!("launching visible time: {:?}", host.active_duration());

    let now = host.now();
    shared
        .set_phase_at(AppPhase::ForegroundActive, now)
        .expect("ui thread");
    host.advance(Duration::from_secs(3));
    println!("foreground union: {:?}", host.active_duration());

    host.background();
    host.advance(Duration::from_secs(4));
    println!("paused union: {:?}", host.active_duration());

    host.resume();
    host.advance(Duration::from_secs(1));
    println!("resumed union: {:?}", host.active_duration());

    let current = SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        text_scale: PreferenceValue::Known(TextScale::new(1.5).expect("valid scale")),
        ..SystemPreferences::default()
    };
    shared.set_preferences(current).expect("ui thread");
    println!("preference events: {}", drain_ready(&preferences).len());

    let v1 = SchemaVersion(1);
    let v2 = SchemaVersion(2);
    let v3 = SchemaVersion(3);
    let bump = |to: SchemaVersion| {
        move |_: SchemaVersion, payload: &[u8]| -> Result<Vec<u8>, SystemError> {
            let mut snapshot = VersionedSnapshot::decode(payload)?;
            snapshot.schema = to;
            Ok(snapshot.encode())
        }
    };
    let mut legacy = VersionedSnapshot::new(v1, 1, RouteIdentity::new("settings"));
    legacy.focus = Some(FocusTarget::new("display-name"));
    legacy.drafts.insert(
        "display-name".to_string(),
        FieldDraft::new("display-name")
            .with_text("Ada", "Ada L.")
            .with_selection(SelectionRange::new(3, 6), SelectionRange::new(8, 14)),
    );
    let mut store = InMemorySnapshotStore::new();
    store.insert_raw("session", legacy.encode());
    let mut coordinator = RestorationCoordinator::new(v3, store)
        .with_migration(v1, v2, bump(v2))
        .with_migration(v2, v3, bump(v3));
    let outcome = coordinator.restore("session", 0);
    println!("multi-hop restoration: {outcome:?}");
    if let RestoreOutcome::Migrated { from, snapshot } = outcome {
        println!("migrated {from:?} -> {:?}", snapshot.schema);
    }

    let mut lifecycle_events = drain_ready(&lifecycle_a);
    lifecycle_events.extend(drain_ready(&lifecycle_b));
    println!("lifecycle subscriber events: {}", lifecycle_events.len());
    println!(
        "resume revalidation events: {} / {}",
        revalidation_a.drain().len(),
        revalidation_b.drain().len()
    );
    println!("session lock: {:?}", shared.session_lock());
    println!("conservative provider entries: {}", PROVIDER_MANIFEST.len());
}
