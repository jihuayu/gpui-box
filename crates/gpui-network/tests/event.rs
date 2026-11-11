mod support;

use gpui_network::*;
use std::sync::Arc;
use support::*;

#[test]
fn bounded_stream_reports_overflow_and_acknowledges_a_snapshot() {
    let (_, service) = fake_service();
    let subscription = service.subscribe();
    for index in 0..100u64 {
        let transport = TransportKind::ALL[(index as usize) % TransportKind::ALL.len()];
        let snapshot = routable(transport)
            .with_cost(if index % 2 == 0 {
                ConnectionCost::Unmetered
            } else {
                ConnectionCost::Metered
            })
            .with_revision(index + 1);
        service.apply_snapshot(snapshot).expect("transition");
    }
    assert!(subscription.snapshot_required());
    let events = drain(&subscription);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ResyncRequired))
    );
    assert!(!values(events).is_empty());
    assert!(subscription.snapshot_required());
    subscription.acknowledge_snapshot(u64::MAX);
    assert!(!subscription.snapshot_required());
}

#[test]
fn capacity_one_evicts_oldest_events_without_blocking() {
    let fake = FakeNetworkProvider::with_snapshot(routable(TransportKind::Ethernet));
    let service =
        NetworkService::with_platform_and_event_capacity(Arc::new(fake), PlatformTarget::Linux, 1);
    let subscription = service.subscribe();
    service
        .apply_snapshot(routable(TransportKind::WiFi))
        .expect("first");
    service
        .apply_snapshot(routable(TransportKind::Cellular))
        .expect("second");
    let events = drain(&subscription);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ResyncRequired))
    );
    assert!(!values(events).is_empty());
}

#[test]
fn default_capacity_is_sixty_four() {
    assert_eq!(DEFAULT_NETWORK_EVENT_CAPACITY, 64);
}

#[test]
fn event_delivery_is_non_blocking_and_stops_after_receiver_drop() {
    let (_, service) = fake_service();
    let subscription = service.subscribe();
    drop(subscription);
    for transport in TransportKind::ALL {
        service
            .apply_snapshot(routable(transport))
            .expect("producer never blocks");
    }
}

#[test]
fn snapshot_refresh_returns_the_normalized_monotonic_revision() {
    let (fake, service) = fake_service();
    let baseline = service.current_snapshot().revision;

    for revision in [0, baseline] {
        let mut candidate = routable(TransportKind::WiFi);
        candidate.revision = revision;
        fake.set_snapshot(candidate);

        let refreshed = completed(service.snapshot(&context()));
        assert_eq!(
            refreshed.revision,
            service.current_snapshot().revision,
            "snapshot() must return exactly the cached normalized revision"
        );
        assert!(
            refreshed.revision > revision,
            "a zero or non-advancing provider revision must be normalized"
        );
    }
}
