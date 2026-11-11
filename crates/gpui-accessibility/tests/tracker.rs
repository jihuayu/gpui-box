mod common;

use accesskit::{Action, NodeId, Role};
use common::*;
use gpui_accessibility::{
    IdentityReplacement, SceneRevision, SemanticError, SemanticSceneTracker, SemanticTreeSnapshot,
};

fn initial_update() -> accesskit::TreeUpdate {
    let mut root = base_node(1, Role::Window);
    root.set_children([NodeId(2)]);
    let mut panel = base_node(2, Role::Group);
    panel.set_children([NodeId(3), NodeId(4)]);
    let mut button = base_node(3, Role::Button);
    author(&mut button, "cta");
    button.add_action(Action::Click);
    button.add_action(Action::Focus);
    let label = base_node(4, Role::Label);
    update(1, 3, vec![(1, root), (2, panel), (3, button), (4, label)])
}

#[test]
fn incremental_updates_prune_unreachable_nodes_and_track_identity() {
    let mut tracker = SemanticSceneTracker::new();
    let first = tracker
        .ingest_accesskit(scene(1), initial_update())
        .expect("initial update");
    assert_eq!(first.nodes.len(), 4);
    assert_eq!(
        first.focus.as_ref().expect("focus").author_id.as_deref(),
        Some("cta")
    );

    let mut panel = base_node(2, Role::Group);
    panel.set_children([NodeId(3)]);
    let orphan = base_node(5, Role::Label);
    let second = tracker
        .ingest_accesskit(scene(2), incremental(3, vec![(2, panel), (5, orphan)]))
        .expect("incremental update");
    assert_eq!(second.nodes.len(), 3);
    assert!(
        second.node_by_node_id(4).is_none(),
        "removed child is pruned"
    );
    assert!(second.node_by_node_id(5).is_none(), "orphan is pruned");
    let audit = tracker.identity_audit(SURFACE).expect("identity audit");
    assert_eq!(audit.revision, SceneRevision(2));
    assert_eq!(audit.retained_count(), 3);
    assert_eq!(
        audit.churned,
        vec![gpui_accessibility::SemanticIdentity::at(4u64)]
    );

    let mut panel = base_node(2, Role::Group);
    panel.set_children([NodeId(6)]);
    let mut rekeyed = base_node(6, Role::Button);
    author(&mut rekeyed, "cta");
    rekeyed.add_action(Action::Click);
    rekeyed.add_action(Action::Focus);
    let third = tracker
        .ingest_accesskit(scene(3), incremental(6, vec![(2, panel), (6, rekeyed)]))
        .expect("relocated author identity");
    assert!(third.node_by_node_id(3).is_none());
    assert_eq!(
        third
            .node_by_node_id(6)
            .expect("relocated button")
            .identity
            .author_id
            .as_deref(),
        Some("cta")
    );
    let audit = tracker.identity_audit(SURFACE).expect("identity audit");
    assert_eq!(audit.relocated.len(), 1);
    assert_eq!(audit.relocated[0].previous.node_id, 3);
    assert_eq!(audit.relocated[0].current.node_id, 6);
    assert!(
        audit.retained.iter().any(|identity| {
            identity.node_id == 6 && identity.author_id.as_deref() == Some("cta")
        })
    );

    let mut panel = base_node(2, Role::Group);
    panel.set_children([NodeId(6)]);
    let mut replaced = base_node(6, Role::Button);
    author(&mut replaced, "different-control");
    replaced.add_action(Action::Click);
    tracker
        .ingest_accesskit(scene(4), incremental(6, vec![(2, panel), (6, replaced)]))
        .expect("replaced identity");
    let audit = tracker.identity_audit(SURFACE).expect("identity audit");
    assert_eq!(
        audit.replaced,
        vec![IdentityReplacement {
            node_id: 6,
            previous: gpui_accessibility::SemanticIdentity::new(6, Some("cta".into()))
                .expect("identity"),
            current: gpui_accessibility::SemanticIdentity::new(6, Some("different-control".into()))
                .expect("identity"),
        }]
    );
    assert!(!audit.stable());
}

#[test]
fn stale_generations_and_revisions_are_rejected_then_reset() {
    let mut tracker = SemanticSceneTracker::new();
    tracker
        .ingest_accesskit(scene(1), initial_update())
        .expect("initial update");

    assert!(matches!(
        tracker.ingest_accesskit(scene(1), initial_update()),
        Err(SemanticError::StaleRevision { .. })
    ));
    assert!(matches!(
        tracker.ingest_accesskit(scene(0), initial_update()),
        Err(SemanticError::StaleRevision { .. })
    ));
    assert!(matches!(
        tracker.ingest_accesskit(scene_generation(0, 2), initial_update()),
        Err(SemanticError::StaleGeneration { .. })
    ));

    let mut root = base_node(1, Role::Window);
    root.set_children([NodeId(2)]);
    let child = base_node(2, Role::Button);
    let generation_two = tracker
        .ingest_accesskit(
            scene_generation(2, 1),
            update(1, 2, vec![(1, root), (2, child)]),
        )
        .expect("new generation resets state");
    assert_eq!(generation_two.generation, 2);
    assert_eq!(generation_two.nodes.len(), 2);
    assert_eq!(tracker.active_generation(SURFACE), Some(2));

    tracker.dispose_surface(SURFACE);
    assert!(tracker.snapshot(SURFACE).is_none());
    assert!(tracker.is_disposed(SURFACE));
    assert!(matches!(
        tracker.ingest_accesskit(scene_generation(2, 2), initial_update()),
        Err(SemanticError::SurfaceDisposed { .. })
    ));

    let mut root = base_node(1, Role::Window);
    root.set_children([]);
    let restored = tracker
        .ingest_accesskit(scene_generation(3, 1), update(1, 1, vec![(1, root)]))
        .expect("a newer surface generation may start again");
    assert_eq!(restored.generation, 3);
    assert_eq!(tracker.active_generation(SURFACE), Some(3));
}

#[test]
fn semantic_commit_validates_and_supports_following_incremental_updates() {
    let mut tracker = SemanticSceneTracker::new();
    let committed = SemanticTreeSnapshot::from_accesskit_update(scene(1), &initial_update())
        .expect("accesskit snapshot");
    tracker.commit(committed.clone()).expect("semantic commit");
    assert_eq!(tracker.snapshot(SURFACE).expect("snapshot").nodes.len(), 4);

    let mut panel = base_node(2, Role::Group);
    panel.set_children([NodeId(3)]);
    let second = tracker
        .ingest_accesskit(scene(2), incremental(3, vec![(2, panel)]))
        .expect("incremental after semantic commit");
    assert_eq!(second.nodes.len(), 3);
    assert!(second.node_by_node_id(4).is_none());

    let mut broken = committed;
    broken.nodes.retain(|node| node.identity.node_id != 1);
    assert!(matches!(
        tracker.commit(broken),
        Err(SemanticError::RootNotInTree { .. })
    ));
}

#[test]
fn scene_validation_requires_exact_revision_and_disposal_is_terminal() {
    let mut tracker = SemanticSceneTracker::new();
    let snapshot = tracker
        .ingest_accesskit(scene(3), initial_update())
        .expect("initial update");
    tracker.validate_scene(scene(3)).expect("current scene");
    assert!(matches!(
        tracker.validate_scene(scene(4)),
        Err(SemanticError::StaleRevision { .. })
    ));

    tracker
        .advance_generation(SURFACE, 2)
        .expect("generation bump");
    assert!(tracker.snapshot(SURFACE).is_none());
    assert!(matches!(
        tracker.validate_scene(scene(3)),
        Err(SemanticError::StaleGeneration { .. })
    ));
    assert_eq!(snapshot.revision, SceneRevision(3));
}
