mod common;

use accesskit::{NodeId, Role};
use common::*;
use gpui_accessibility::{
    AccessibilityPlatform, AccessibilityService, FakeAccessibilityProvider, LiveAnnouncement,
    LiveAnnouncementDisposition, LiveAnnouncementQueue, LivePriority, SceneRevision,
    SemanticIdentity, SemanticRole, SemanticTreeSnapshot, VirtualListSemanticsAudit,
    VirtualListViolation,
};
use gpui_system::UiOwner;
use std::rc::Rc;

struct ListFixture {
    tree: SemanticTreeSnapshot,
    list: SemanticIdentity,
    selection: Option<SemanticIdentity>,
}

fn list_tree(
    revision: u64,
    items: &[(u64, Option<&str>, usize, f64)],
    size: usize,
    viewport: (f64, f64, f64, f64),
    extra_root_children: &[u64],
) -> ListFixture {
    let mut nodes = vec![semantic_node(1, None, SemanticRole::Window)];
    let mut list = semantic_node(2, Some("results"), SemanticRole::List);
    list.bounds = Some(bounds(viewport.0, viewport.1, viewport.2, viewport.3));
    nodes.push(list);
    for (id, author_id, index, y) in items {
        let mut item = semantic_node(*id, *author_id, SemanticRole::ListItem);
        item.index_in_set = Some(*index);
        item.size_of_set = Some(size);
        item.bounds = Some(bounds(0.0, *y, 200.0, 40.0));
        nodes.push(item);
    }
    for id in extra_root_children {
        nodes.push(semantic_node(*id, None, SemanticRole::ListItem));
        link(&mut nodes, 1, *id);
    }
    link(&mut nodes, 1, 2);
    for (id, _, _, _) in items {
        link(&mut nodes, 2, *id);
    }
    let focus = items.first().map_or(2, |(id, _, _, _)| *id);
    let tree = snapshot(scene(revision), 1, focus, nodes);
    let selection = items
        .iter()
        .find(|(_, author_id, _, _)| *author_id == Some("row-1"))
        .map(|(id, _, _, _)| {
            SemanticIdentity::new(*id, Some("row-1".into())).expect("stable selection")
        });
    ListFixture {
        tree,
        list: SemanticIdentity::new(2, Some("results".into())).expect("list"),
        selection,
    }
}

#[test]
fn virtual_list_retains_selection_across_scroll_filter_and_presentation_order() {
    let first = list_tree(
        1,
        &[
            (10, None, 0, 0.0),
            (11, Some("row-1"), 1, 40.0),
            (12, None, 2, 80.0),
        ],
        100,
        (0.0, 0.0, 400.0, 180.0),
        &[],
    );
    let mut audit = VirtualListSemanticsAudit::new(first.list.clone(), first.selection.clone());
    let report = audit.audit(&first.tree).expect("first list audit");
    assert!(report.passed(), "{:?}", report.violations);
    assert_eq!(report.visible_indices, vec![0, 1, 2]);
    assert_eq!(report.visible_range, Some((0, 2)));
    assert!(report.selection_visible);
    assert!(report.selection_retained);

    let scrolled = list_tree(
        2,
        &[
            (13, None, 20, 0.0),
            (14, None, 21, 40.0),
            (15, None, 22, 80.0),
        ],
        100,
        (0.0, 0.0, 400.0, 180.0),
        &[],
    );
    let report = audit.audit(&scrolled.tree).expect("scrolled audit");
    assert!(report.passed(), "{:?}", report.violations);
    assert!(!report.selection_visible);
    assert!(
        report.selection_retained,
        "a selection outside the moved window is temporarily scrolled away"
    );

    let filtered = list_tree(
        3,
        &[(17, None, 4, 0.0), (16, Some("row-1"), 3, 40.0)],
        50,
        (0.0, 0.0, 400.0, 180.0),
        &[],
    );
    let report = audit.audit(&filtered.tree).expect("filtered audit");
    assert!(report.passed(), "{:?}", report.violations);
    assert_eq!(report.visible_indices, vec![4, 3]);
    assert_eq!(report.visible_range, Some((3, 4)));
    assert!(report.selection_visible);
    assert!(report.selection_retained);
    assert_eq!(report.size_of_set, Some(50));

    let offscreen = list_tree(
        4,
        &[
            (16, Some("row-1"), 3, 40.0),
            (17, None, 4, 0.0),
            (18, None, 6, 1_000.0),
        ],
        50,
        (0.0, 0.0, 400.0, 180.0),
        &[],
    );
    let report = audit.audit(&offscreen.tree).expect("offscreen audit");
    assert!(!report.passed());
    assert!(report.offscreen.iter().any(|node| node.node_id == 18));
    assert!(
        report
            .violations
            .contains(&VirtualListViolation::OffscreenItem(SemanticIdentity::at(
                18u64
            )))
    );

    let orphan = list_tree(
        5,
        &[(16, Some("row-1"), 3, 40.0), (17, None, 4, 0.0)],
        50,
        (0.0, 0.0, 400.0, 180.0),
        &[19],
    );
    let report = audit.audit(&orphan.tree).expect("orphan audit");
    assert!(!report.passed());
    assert_eq!(report.orphans, vec![SemanticIdentity::at(19u64)]);
}

#[test]
fn virtual_list_rejects_invalid_index_metadata_and_missing_bounds() {
    let mut fixture = list_tree(
        1,
        &[(10, Some("row-1"), 0, 0.0)],
        10,
        (0.0, 0.0, 400.0, 180.0),
        &[],
    );
    let mut audit = VirtualListSemanticsAudit::new(fixture.list.clone(), None);
    fixture.tree.nodes[2].index_in_set = Some(200);
    let report = audit.audit(&fixture.tree).expect("invalid index");
    assert!(report.violations.iter().any(|violation| matches!(
        violation,
        VirtualListViolation::InvalidIndex {
            index: 200,
            size: 10,
            ..
        }
    )));

    fixture.tree.nodes[2].index_in_set = None;
    let report = audit.audit(&fixture.tree).expect("missing metadata");
    assert!(
        report
            .violations
            .iter()
            .any(|violation| matches!(violation, VirtualListViolation::MissingSetMetadata(_)))
    );

    fixture.tree.nodes[2].index_in_set = Some(0);
    fixture.tree.nodes[2].bounds = None;
    let report = audit.audit(&fixture.tree).expect("missing bounds");
    assert!(
        report
            .violations
            .iter()
            .any(|violation| matches!(violation, VirtualListViolation::MissingBounds(_)))
    );
}

#[test]
fn live_queue_deduplicates_bounds_and_revokes_on_generation_or_disposal() {
    let mut queue = LiveAnnouncementQueue::new(2);
    let source = SemanticIdentity::new(9, Some("status".into())).expect("source");
    let saved = LiveAnnouncement::new(scene(1), source.clone(), LivePriority::Polite, "Saved");
    assert_eq!(
        queue.enqueue(saved.clone()),
        Ok(LiveAnnouncementDisposition::Queued)
    );
    assert_eq!(
        queue.enqueue(saved),
        Ok(LiveAnnouncementDisposition::Deduplicated)
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.dropped_count(), 0);

    let warning = LiveAnnouncement::new(
        scene(1),
        source.clone(),
        LivePriority::Assertive,
        "Connection lost",
    );
    assert_eq!(
        queue.enqueue(warning),
        Ok(LiveAnnouncementDisposition::Queued)
    );
    let third = LiveAnnouncement::new(scene(2), source.clone(), LivePriority::Polite, "Retrying");
    assert_eq!(
        queue.enqueue(third),
        Ok(LiveAnnouncementDisposition::Queued)
    );
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.dropped_count(), 1);
    let drained = queue.drain();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].message, "Connection lost");
    assert_eq!(drained[1].message, "Retrying");

    assert_eq!(
        queue.advance_generation(scene_generation(2, 1)),
        Ok(0),
        "old generation was already drained"
    );
    let generation_two = LiveAnnouncement::new(
        scene_generation(2, 2),
        source.clone(),
        LivePriority::Polite,
        "Done",
    );
    assert_eq!(
        queue.enqueue(generation_two),
        Ok(LiveAnnouncementDisposition::Queued)
    );
    let stale = LiveAnnouncement::new(
        scene_generation(2, 1),
        source.clone(),
        LivePriority::Polite,
        "Stale",
    );
    assert!(matches!(
        queue.enqueue(stale),
        Err(gpui_accessibility::SemanticError::StaleRevision { .. })
    ));
    assert_eq!(queue.dispose_surface(SURFACE.0), 1);
    assert!(queue.is_empty());
    assert!(matches!(
        queue.enqueue(LiveAnnouncement::new(
            scene_generation(2, 3),
            source,
            LivePriority::Polite,
            "Gone"
        )),
        Err(gpui_accessibility::SemanticError::SurfaceDisposed { .. })
    ));
}

#[test]
fn service_announcements_are_bounded_deduplicated_and_disposal_cleared() {
    let provider = Rc::new(FakeAccessibilityProvider::new(
        AccessibilityPlatform::Windows,
    ));
    let service = AccessibilityService::with_live_capacity(provider.clone(), 2);
    let owner = UiOwner::new(SURFACE);
    let mut root = base_node(1, Role::Window);
    root.set_children(Vec::<NodeId>::new());
    let mut commit = service.commit_scene(
        SemanticTreeSnapshot::from_accesskit_update(scene(1), &update(1, 1, vec![(1, root)]))
            .expect("scene"),
        ui_context(&owner),
    );
    match poll_once(&mut commit) {
        std::task::Poll::Ready(result) => {
            completed(result);
        }
        std::task::Poll::Pending => panic!("commit did not complete"),
    }

    let source = service.snapshot(SURFACE).expect("snapshot").root.clone();
    let saved = LiveAnnouncement::new(scene(1), source.clone(), LivePriority::Polite, "Saved");
    let mut first = service.announce(saved.clone(), ui_context(&owner));
    match poll_once(&mut first) {
        std::task::Poll::Ready(result) => {
            let receipt = completed(result);
            assert_eq!(receipt.disposition, LiveAnnouncementDisposition::Queued);
            assert_eq!(receipt.queue_len, 1);
        }
        std::task::Poll::Pending => panic!("announcement did not complete"),
    }
    let mut duplicate = service.announce(saved, ui_context(&owner));
    match poll_once(&mut duplicate) {
        std::task::Poll::Ready(result) => {
            let receipt = completed(result);
            assert_eq!(
                receipt.disposition,
                LiveAnnouncementDisposition::Deduplicated
            );
            assert_eq!(receipt.queue_len, 1);
        }
        std::task::Poll::Pending => panic!("announcement did not complete"),
    }
    assert_eq!(provider.recorded_announcements().len(), 1);

    let mut third = service.announce(
        LiveAnnouncement::new(scene(1), source.clone(), LivePriority::Polite, "Third"),
        ui_context(&owner),
    );
    match poll_once(&mut third) {
        std::task::Poll::Ready(result) => {
            let receipt = completed(result);
            assert_eq!(receipt.queue_len, 2);
            assert_eq!(receipt.dropped, 0);
        }
        std::task::Poll::Pending => panic!("announcement did not complete"),
    }
    let mut fourth = service.announce(
        LiveAnnouncement::new(scene(1), source.clone(), LivePriority::Assertive, "Fourth"),
        ui_context(&owner),
    );
    match poll_once(&mut fourth) {
        std::task::Poll::Ready(result) => {
            let receipt = completed(result);
            assert_eq!(receipt.queue_len, 2);
            assert_eq!(receipt.dropped, 1);
        }
        std::task::Poll::Pending => panic!("announcement did not complete"),
    }
    assert_eq!(service.live_dropped_count(), 1);

    service.dispose_surface(SURFACE);
    assert_eq!(service.live_queue_len(), 0);
    let mut after_disposal = service.announce(
        LiveAnnouncement::new(
            scene_generation(1, 2),
            source,
            LivePriority::Polite,
            "After disposal",
        ),
        ui_context(&owner),
    );
    match poll_once(&mut after_disposal) {
        std::task::Poll::Ready(Err(error)) => {
            assert_eq!(error.code, gpui_system::ErrorCode::ContextExpired)
        }
        other => panic!("announcement after disposal was not rejected: {other:?}"),
    }
}

#[test]
fn live_queue_rejects_empty_messages_and_tracks_scene_revision() {
    let mut queue = LiveAnnouncementQueue::new(1);
    let source = SemanticIdentity::at(1u64);
    assert!(matches!(
        queue.enqueue(LiveAnnouncement::new(
            scene(1),
            source.clone(),
            LivePriority::Polite,
            "  "
        )),
        Err(gpui_accessibility::SemanticError::InvalidAnnouncement)
    ));
    let mut queue_revision = LiveAnnouncementQueue::new(1);
    let announcement = LiveAnnouncement::new(scene(1), source, LivePriority::Polite, "Message");
    assert_eq!(announcement.scene.revision, SceneRevision(1));
    assert_eq!(
        queue_revision.enqueue(announcement),
        Ok(LiveAnnouncementDisposition::Queued)
    );
}
