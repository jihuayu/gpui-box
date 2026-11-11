mod common;

use accesskit::{Action, NodeId, Rect, Role};
use common::*;
use gpui_accessibility::{
    AccessibilityOperation, AccessibilityService, BoundsError, FakeAccessibilityProvider,
    SceneRevision, SemanticAction, SemanticActionPayload, SemanticActionRequest, SemanticBounds,
    SemanticRole, TextSelectionError, TextSelectionRequest, TextSelectionSpan, Utf8Selection,
    Utf16Selection,
};
use gpui_system::{Completion, ErrorCode, SurfaceId, UiOwner};
use std::rc::Rc;

#[test]
fn directed_selection_roundtrips_utf8_scalar_and_utf16() {
    for text in ["纪要 ✅", "Primary draft 你好 ✅", "a😀b✅"] {
        let scalar_len = text.chars().count();
        let utf8_len = text.len();
        let utf16_len: usize = text.chars().map(char::len_utf16).sum();

        let forward = TextSelectionSpan::new(text, 1, scalar_len.min(3)).expect("forward span");
        let from_utf8 =
            TextSelectionSpan::from_utf8(text, forward.to_utf8(text).expect("utf8 span"))
                .expect("utf8 conversion");
        let from_utf16 =
            TextSelectionSpan::from_utf16(text, forward.to_utf16(text).expect("utf16 span"))
                .expect("utf16 conversion");
        assert_eq!(from_utf8, forward);
        assert_eq!(from_utf16, forward);
        assert_eq!(forward.scalar().anchor, 1);
        assert_eq!(forward.scalar().focus, scalar_len.min(3));
        assert_eq!(forward.slice(text).expect("slice"), {
            let start = text.char_indices().nth(1).expect("start").0;
            let end = text
                .char_indices()
                .nth(scalar_len.min(3))
                .map_or(text.len(), |(byte, _)| byte);
            &text[start..end]
        });

        let backward = TextSelectionSpan::new(text, scalar_len.min(3), 1).expect("backward span");
        assert!(backward.is_reversed());
        assert_eq!(
            backward.slice(text).expect("ordered slice"),
            forward.slice(text).expect("slice")
        );
        assert!(
            backward
                .to_utf16(text)
                .expect("utf16 backward")
                .is_reversed()
        );

        let collapsed = TextSelectionSpan::collapsed_at(text, 0).expect("collapsed span");
        assert!(collapsed.is_collapsed());
        assert_eq!(collapsed.to_utf8(text), Ok(Utf8Selection::collapsed(0)));
        assert_eq!(collapsed.to_utf16(text), Ok(Utf16Selection::collapsed(0)));

        let whole = TextSelectionSpan::new(text, 0, scalar_len).expect("whole span");
        assert_eq!(
            whole.to_utf8(text).expect("whole utf8"),
            Utf8Selection::new(0, utf8_len)
        );
        assert_eq!(
            whole.to_utf16(text).expect("whole utf16"),
            Utf16Selection::new(0, utf16_len)
        );
        assert_eq!(
            TextSelectionSpan::from_utf16(text, Utf16Selection::new(0, utf16_len))
                .expect("whole from utf16"),
            whole
        );

        let accesskit = whole.to_accesskit(NodeId(9));
        assert_eq!(
            TextSelectionSpan::from_accesskit(text, &accesskit).expect("accesskit span"),
            whole
        );
        assert_eq!(
            TextSelectionSpan::from_accesskit_for_node(text, &accesskit, NodeId(8)),
            Err(TextSelectionError::AccessKitNodeMismatch {
                expected: 8,
                found: 9
            })
        );
    }
}

#[test]
fn selection_rejects_out_of_range_mid_scalar_and_mid_surrogate_offsets() {
    let text = "纪要 ✅";
    assert_eq!(
        TextSelectionSpan::from_utf8(text, Utf8Selection::new(1, 1)),
        Err(TextSelectionError::Utf8NotCharBoundary { offset: 1 })
    );
    assert_eq!(
        TextSelectionSpan::from_utf8(text, Utf8Selection::new(0, 100)),
        Err(TextSelectionError::Utf8OutOfRange {
            offset: 100,
            byte_len: text.len()
        })
    );
    assert_eq!(
        TextSelectionSpan::from_utf16(text, Utf16Selection::new(0, 100)),
        Err(TextSelectionError::Utf16OutOfRange {
            offset: 100,
            utf16_len: 4
        })
    );
    assert_eq!(
        TextSelectionSpan::new(text, 0, 5),
        Err(TextSelectionError::ScalarOutOfRange {
            offset: 5,
            scalar_len: 4
        })
    );

    let emoji = "a😀b";
    assert_eq!(
        TextSelectionSpan::from_utf16(emoji, Utf16Selection::new(2, 2)),
        Err(TextSelectionError::Utf16NotCodeUnitBoundary { offset: 2 })
    );
    assert_eq!(
        TextSelectionSpan::from_utf16(emoji, Utf16Selection::new(0, 1))
            .expect("before the surrogate pair"),
        TextSelectionSpan::new(emoji, 0, 1).expect("scalar one")
    );
    assert_eq!(
        TextSelectionSpan::from_utf16(emoji, Utf16Selection::new(0, 3))
            .expect("after the surrogate pair"),
        TextSelectionSpan::new(emoji, 0, 2).expect("scalar two")
    );
}

#[test]
fn bounds_require_finite_non_inverted_rectangles() {
    assert_eq!(
        SemanticBounds::new(f64::NAN, 0.0, 1.0, 1.0),
        Err(BoundsError::NonFinite)
    );
    assert_eq!(
        SemanticBounds::new(0.0, 0.0, f64::INFINITY, 1.0),
        Err(BoundsError::NonFinite)
    );
    assert_eq!(
        SemanticBounds::new(10.0, 0.0, 5.0, 1.0),
        Err(BoundsError::Inverted)
    );
    assert_eq!(
        SemanticBounds::new(0.0, 10.0, 1.0, 5.0),
        Err(BoundsError::Inverted)
    );
    assert_eq!(
        SemanticBounds::from_origin_size(0.0, 0.0, -1.0, 2.0),
        Err(BoundsError::Inverted)
    );
    assert_eq!(
        SemanticBounds::from_accesskit_rect(Rect::new(0.0, 0.0, f64::NAN, 1.0)),
        Err(BoundsError::NonFinite)
    );

    let valid =
        SemanticBounds::from_accesskit_rect(Rect::new(1.0, 2.0, 11.0, 22.0)).expect("valid bounds");
    assert_eq!(valid.width(), 10.0);
    assert_eq!(valid.height(), 20.0);
    assert_eq!(valid.accesskit_rect(), Rect::new(1.0, 2.0, 11.0, 22.0));
    assert!(valid.intersects(bounds(10.0, 20.0, 5.0, 5.0)));
    assert!(!valid.intersects(bounds(100.0, 100.0, 5.0, 5.0)));
}

#[test]
fn action_requests_enforce_typed_payloads() {
    let displayed = scene(1);
    let target = gpui_accessibility::SemanticIdentity::at(3u64);
    for request in [
        SemanticActionRequest::click(displayed, target.clone()),
        SemanticActionRequest::focus(displayed, target.clone()),
        SemanticActionRequest::scroll_into_view(displayed, target.clone()),
        SemanticActionRequest::dismiss(displayed, target.clone()),
    ] {
        assert_eq!(request.payload, SemanticActionPayload::None);
        assert!(request.validate().is_ok());
    }
    let set_value = SemanticActionRequest::set_value(displayed, target.clone(), "你好");
    assert_eq!(set_value.value(), Some("你好"));
    assert!(set_value.validate().is_ok());

    let selection = TextSelectionSpan::new("纪要 ✅", 0, 2).expect("selection");
    let set_selection = SemanticActionRequest::set_text_selection(displayed, target, selection);
    assert_eq!(set_selection.selection(), Some(selection));
    assert!(set_selection.validate().is_ok());

    let invalid = SemanticActionRequest {
        scene: displayed,
        target: gpui_accessibility::SemanticIdentity::at(3u64),
        action: SemanticAction::SetValue,
        payload: SemanticActionPayload::None,
    };
    assert_eq!(
        invalid.validate(),
        Err(gpui_accessibility::SemanticError::InvalidActionPayload {
            action: SemanticAction::SetValue
        })
    );
    assert_eq!(
        SemanticAction::from_accesskit(Action::SetTextSelection),
        Some(SemanticAction::SetTextSelection)
    );
    assert_eq!(SemanticAction::Dismiss.accesskit_action(), None);
}

fn service_fixture() -> (
    Rc<FakeAccessibilityProvider>,
    AccessibilityService,
    TextSelectionSpan,
) {
    let text = "纪要 ✅";
    let selection = TextSelectionSpan::new(text, 0, 2).expect("selection");
    let mut root = base_node(1, Role::Window);
    root.set_children([NodeId(2), NodeId(3), NodeId(4), NodeId(5)]);
    let mut button = base_node(2, Role::Button);
    button.add_action(Action::Click);
    button.add_action(Action::Focus);
    let mut input = base_node(3, Role::TextInput);
    input.set_value(text);
    input.set_placeholder("Draft");
    input.add_action(Action::Focus);
    input.add_action(Action::SetValue);
    input.add_action(Action::SetTextSelection);
    let mut item = base_node(4, Role::ListItem);
    item.add_action(Action::ScrollIntoView);
    let mut dialog = base_node(5, Role::Dialog);
    dialog.add_action(Action::Click);
    let mut snapshot = gpui_accessibility::SemanticTreeSnapshot::from_accesskit_update(
        scene(1),
        &update(
            1,
            2,
            vec![(1, root), (2, button), (3, input), (4, item), (5, dialog)],
        ),
    )
    .expect("scene snapshot");
    let dialog_node = snapshot
        .nodes
        .iter_mut()
        .find(|node| node.identity.node_id == 5)
        .expect("dialog");
    dialog_node.actions = vec![SemanticAction::Dismiss];
    dialog_node.states.modal = true;

    let provider = Rc::new(FakeAccessibilityProvider::new(
        gpui_accessibility::AccessibilityPlatform::Windows,
    ));
    let service = AccessibilityService::new(provider.clone());
    let owner = UiOwner::new(SURFACE);
    let mut request = service.commit_scene(snapshot, ui_context(&owner));
    let committed = completed(match poll_once(&mut request) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("commit did not complete synchronously"),
    });
    assert_eq!(committed.nodes.len(), 5);
    (provider, service, selection)
}

#[test]
fn service_dispatches_all_actions_and_records_directed_text_edits() {
    let (provider, service, selection) = service_fixture();
    let owner = UiOwner::new(SURFACE);
    let scene = scene(1);
    let target = |id| {
        service
            .snapshot(SURFACE)
            .expect("snapshot")
            .node_by_node_id(id)
            .expect("node")
            .identity
            .clone()
    };
    let requests = [
        SemanticActionRequest::click(scene, target(2)),
        SemanticActionRequest::focus(scene, target(2)),
        SemanticActionRequest::set_value(scene, target(3), "Updated"),
        SemanticActionRequest::set_text_selection(scene, target(3), selection),
        SemanticActionRequest::scroll_into_view(scene, target(4)),
        SemanticActionRequest::dismiss(scene, target(5)),
    ];
    for request in requests {
        let mut future = service.perform_action(request, ui_context(&owner));
        match poll_once(&mut future) {
            std::task::Poll::Ready(result) => {
                assert_eq!(completed(result).scene, scene);
            }
            std::task::Poll::Pending => panic!("action did not complete synchronously"),
        }
    }
    assert_eq!(provider.dispatched_actions().len(), 6);
    assert_eq!(
        provider.dispatched_actions()[3].payload,
        SemanticActionPayload::TextSelection(selection)
    );

    let text = "纪要 ✅";
    let edit = TextSelectionRequest::new(scene, target(3), text, selection);
    let mut future = service.edit_text_selection(edit, ui_context(&owner));
    match poll_once(&mut future) {
        std::task::Poll::Ready(result) => {
            let receipt = completed(result);
            assert_eq!(receipt.utf8, Utf8Selection::new(0, 6));
            assert_eq!(receipt.utf16, Utf16Selection::new(0, 2));
            assert_eq!(
                receipt.scalar,
                gpui_accessibility::ScalarSelection::new(0, 2)
            );
        }
        std::task::Poll::Pending => panic!("selection did not complete synchronously"),
    }
    assert_eq!(provider.recorded_selections().len(), 1);
}

#[test]
fn action_capability_gating_and_cancellation_are_typed() {
    let (provider, service, _) = service_fixture();
    let owner = UiOwner::new(SURFACE);
    let scene = scene(1);
    let target = service
        .snapshot(SURFACE)
        .expect("snapshot")
        .node_by_node_id(2)
        .expect("button")
        .identity
        .clone();

    provider.set_operation_unsupported(AccessibilityOperation::PerformAction);
    let mut future = service.perform_action(
        SemanticActionRequest::click(scene, target.clone()),
        ui_context(&owner),
    );
    match poll_once(&mut future) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::Unsupported),
        other => panic!("unsupported action was not rejected: {other:?}"),
    }

    provider.set_support(
        AccessibilityOperation::PerformAction,
        gpui_system::Support::Native,
    );
    provider.set_operation_unavailable(AccessibilityOperation::PerformAction);
    let mut future = service.perform_action(
        SemanticActionRequest::click(scene, target.clone()),
        ui_context(&owner),
    );
    match poll_once(&mut future) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::Unavailable),
        other => panic!("unavailable action was not rejected: {other:?}"),
    }

    provider.set_availability(
        AccessibilityOperation::PerformAction,
        gpui_system::Availability::Available,
    );
    provider.set_operation_configuration_required(AccessibilityOperation::PerformAction);
    let mut future = service.perform_action(
        SemanticActionRequest::click(scene, target),
        ui_context(&owner),
    );
    match poll_once(&mut future) {
        std::task::Poll::Ready(Err(error)) => {
            assert_eq!(error.code, ErrorCode::InvalidConfiguration)
        }
        other => panic!("misconfigured action was not rejected: {other:?}"),
    }

    let context = ui_context(&owner);
    context.cancellation.cancel(gpui_system::CancelReason::User);
    let mut future = service.observe_tree(
        scene_generation(1, 2),
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        context,
    );
    match poll_once(&mut future) {
        std::task::Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            assert_eq!(reason, gpui_system::CancelReason::User)
        }
        other => panic!("cancellation was not preserved: {other:?}"),
    }
}

#[test]
fn action_target_requires_exact_current_scene() {
    let (_provider, service, _) = service_fixture();
    let owner = UiOwner::new(SURFACE);
    let target = service
        .snapshot(SURFACE)
        .expect("snapshot")
        .node_by_node_id(2)
        .expect("button")
        .identity
        .clone();
    let stale_scene = scene_generation(1, 2);
    let mut future = service.perform_action(
        SemanticActionRequest::click(stale_scene, target),
        ui_context(&owner),
    );
    match poll_once(&mut future) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::ResourceExpired),
        other => panic!("stale action was not rejected: {other:?}"),
    }
}

#[test]
fn semantic_roles_cover_shared_component_contracts() {
    for (role, expected) in [
        (Role::Window, SemanticRole::Window),
        (Role::Group, SemanticRole::Group),
        (Role::Button, SemanticRole::Button),
        (Role::TextInput, SemanticRole::TextInput),
        (Role::MultilineTextInput, SemanticRole::TextInput),
        (Role::Label, SemanticRole::StaticText),
        (Role::CheckBox, SemanticRole::CheckBox),
        (Role::Switch, SemanticRole::Switch),
        (Role::Dialog, SemanticRole::Dialog),
        (Role::List, SemanticRole::List),
        (Role::ListItem, SemanticRole::ListItem),
        (Role::Heading, SemanticRole::Heading),
        (Role::Status, SemanticRole::LiveRegion),
        (Role::ScrollView, SemanticRole::ScrollView),
        (Role::Toolbar, SemanticRole::Toolbar),
        (Role::Tab, SemanticRole::Tab),
        (Role::TabList, SemanticRole::TabList),
        (Role::Image, SemanticRole::Unknown),
    ] {
        assert_eq!(SemanticRole::from_accesskit(role), expected);
    }
    assert_eq!(SemanticRole::BottomSheet.accesskit_role(), Role::Dialog);
    assert_eq!(SceneRevision(3).next(), Some(SceneRevision(4)));
    assert_eq!(SurfaceId(7), SURFACE);
}
