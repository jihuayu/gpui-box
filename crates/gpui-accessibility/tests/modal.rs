mod common;

use common::*;
use gpui_accessibility::{
    ModalIsolationAudit, SemanticAction, SemanticIdentity, SemanticRole, SemanticTreeSnapshot,
};

fn modal_tree(background_disabled: bool) -> SemanticTreeSnapshot {
    let mut nodes = vec![
        semantic_node(1, None, SemanticRole::Window),
        semantic_node(2, Some("open-filter"), SemanticRole::Button),
        semantic_node(3, Some("dialog"), SemanticRole::Dialog),
        semantic_node(4, Some("close"), SemanticRole::Button),
        semantic_node(5, Some("message"), SemanticRole::StaticText),
    ];
    link(&mut nodes, 1, 2);
    link(&mut nodes, 1, 3);
    link(&mut nodes, 3, 4);
    link(&mut nodes, 3, 5);
    nodes[1].states.focusable = true;
    nodes[1].actions = vec![SemanticAction::Click];
    nodes[1].states.disabled = background_disabled;
    nodes[2].states.modal = true;
    nodes[3].states.focusable = true;
    nodes[3].actions = vec![SemanticAction::Click];
    snapshot(scene(1), 1, 4, nodes)
}

#[test]
fn isolated_dialog_passes_but_leaked_background_control_fails() {
    let trigger = SemanticIdentity::new(2, Some("open-filter".into())).expect("trigger");
    let modal = SemanticIdentity::new(3, Some("dialog".into())).expect("modal");
    let audit = ModalIsolationAudit::new(trigger.clone());

    let isolated = modal_tree(true);
    let report = audit.audit(&isolated, &modal).expect("modal report");
    assert!(report.focus_inside_modal);
    assert!(report.modal_present);
    assert_eq!(report.background_focusables.len(), 1);
    assert!(report.leaked_background_controls.is_empty());
    assert!(report.passed());

    let leaked = modal_tree(false);
    let report = audit.audit(&leaked, &modal).expect("modal report");
    assert_eq!(report.leaked_background_controls, vec![trigger]);
    assert!(!report.passed());

    let mut focus_outside = modal_tree(true);
    focus_outside.focus = Some(SemanticIdentity::at(2u64));
    let report = audit
        .audit(&focus_outside, &modal)
        .expect("focus-outside report");
    assert!(!report.focus_inside_modal);
    assert!(!report.passed());
}

#[test]
fn hidden_ancestor_inerts_background_controls() {
    let mut nodes = vec![
        semantic_node(1, None, SemanticRole::Window),
        semantic_node(2, Some("hidden-region"), SemanticRole::Group),
        semantic_node(3, Some("hidden-button"), SemanticRole::Button),
        semantic_node(4, Some("sheet"), SemanticRole::BottomSheet),
        semantic_node(5, Some("close"), SemanticRole::Button),
    ];
    link(&mut nodes, 1, 2);
    link(&mut nodes, 2, 3);
    link(&mut nodes, 1, 4);
    link(&mut nodes, 4, 5);
    nodes[1].states.hidden = true;
    nodes[2].states.focusable = true;
    nodes[2].actions = vec![SemanticAction::Click];
    nodes[3].states.modal = true;
    nodes[4].states.focusable = true;
    nodes[4].actions = vec![SemanticAction::Click];
    let tree = snapshot(scene(1), 1, 5, nodes);
    let trigger = SemanticIdentity::new(3, Some("hidden-button".into())).expect("trigger");
    let modal = SemanticIdentity::new(4, Some("sheet".into())).expect("modal");
    let report = ModalIsolationAudit::new(trigger)
        .audit(&tree, &modal)
        .expect("bottom-sheet report");
    assert!(report.passed());
    assert_eq!(report.background_focusables.len(), 1);
}

#[test]
fn bottom_sheet_audits_and_restores_relocated_trigger_identity() {
    let trigger = SemanticIdentity::new(2, Some("open-filter".into())).expect("trigger");
    let modal = SemanticIdentity::new(3, Some("sheet".into())).expect("modal");
    let audit = ModalIsolationAudit::new(trigger.clone());

    let mut nodes = vec![
        semantic_node(1, None, SemanticRole::Window),
        semantic_node(2, Some("open-filter"), SemanticRole::Button),
        semantic_node(3, Some("sheet"), SemanticRole::BottomSheet),
        semantic_node(4, Some("close"), SemanticRole::Button),
    ];
    link(&mut nodes, 1, 2);
    link(&mut nodes, 1, 3);
    link(&mut nodes, 3, 4);
    nodes[1].states.focusable = true;
    nodes[1].actions = vec![SemanticAction::Click];
    nodes[1].states.disabled = true;
    nodes[2].states.modal = true;
    nodes[2].role = SemanticRole::BottomSheet;
    nodes[3].states.focusable = true;
    nodes[3].actions = vec![SemanticAction::Dismiss];
    let presented = snapshot(scene(2), 1, 4, nodes);
    let report = audit
        .audit(&presented, &modal)
        .expect("presented sheet report");
    assert!(report.passed());
    let dismissal = audit
        .audit_dismissal(&presented, &modal)
        .expect("dismissal report");
    assert!(!dismissal.passed, "the sheet is still present");

    let mut nodes = vec![
        semantic_node(1, None, SemanticRole::Window),
        semantic_node(7, Some("open-filter"), SemanticRole::Button),
    ];
    link(&mut nodes, 1, 7);
    nodes[1].states.focusable = true;
    nodes[1].actions = vec![SemanticAction::Click];
    let dismissed = snapshot(scene(3), 1, 7, nodes);
    let report = audit
        .audit_dismissal(&dismissed, &modal)
        .expect("dismissal report");
    assert!(report.modal_dismissed);
    assert!(report.trigger_retained);
    assert!(report.focus_restored);
    assert!(report.passed);

    let mut focus_elsewhere = dismissed.clone();
    focus_elsewhere.focus = Some(SemanticIdentity::at(1u64));
    let report = audit
        .audit_dismissal(&focus_elsewhere, &modal)
        .expect("dismissal report");
    assert!(!report.focus_restored);
    assert!(!report.passed);
}

#[test]
fn active_modal_discovery_prefers_modal_flag_then_document_order() {
    let mut nodes = vec![
        semantic_node(1, None, SemanticRole::Window),
        semantic_node(2, None, SemanticRole::Dialog),
        semantic_node(3, None, SemanticRole::BottomSheet),
    ];
    link(&mut nodes, 1, 2);
    link(&mut nodes, 1, 3);
    nodes[2].states.modal = true;
    let tree = snapshot(scene(1), 1, 1, nodes);
    assert_eq!(
        ModalIsolationAudit::find_active_modal(&tree),
        Some(SemanticIdentity::at(3u64))
    );
}
