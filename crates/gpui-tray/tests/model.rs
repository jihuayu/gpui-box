mod support;

use gpui_tray::*;
use support::*;

#[test]
fn status_item_keys_are_narrow_ascii_identifiers() {
    for value in ["a", "app.status", "app_status-1", "9"] {
        assert_eq!(key(value).as_str(), value);
    }
    for value in [
        "",
        "with space",
        "slash/value",
        "é",
        "line\nbreak",
        "nul\0byte",
    ] {
        assert!(
            StatusItemKey::new(value).is_err(),
            "{value:?} must be rejected"
        );
    }
    assert!(StatusItemKey::new("a".repeat(64)).is_ok());
    assert!(StatusItemKey::new("a".repeat(65)).is_err());
    let serialized = serde_json::to_string(&key("app.status")).expect("serialize");
    assert_eq!(serialized, "\"app.status\"");
    let parsed: StatusItemKey = serde_json::from_str(&serialized).expect("deserialize");
    assert_eq!(parsed.as_str(), "app.status");
}

#[test]
fn labels_fall_back_deterministically_and_blank_items_are_rejected() {
    let tooltip = StatusItemSpec::new(key("app.status"), icon_set())
        .with_tooltip("Tooltip label")
        .validate()
        .expect("tooltip fallback");
    assert_eq!(tooltip.accessibility_label, "Tooltip label");

    let title = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Title label")
        .with_tooltip("   ")
        .validate()
        .expect("title fallback");
    assert_eq!(title.accessibility_label, "Title label");

    let explicit = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Title")
        .with_accessibility_label("Explicit label")
        .validate()
        .expect("explicit label");
    assert_eq!(explicit.accessibility_label, "Explicit label");

    let unlabeled = StatusItemSpec::new(key("app.status"), icon_set());
    assert_eq!(
        unlabeled.validate().expect_err("unlabeled").code,
        ErrorCode::InvalidInput
    );
    let blank_title = StatusItemSpec::new(key("app.status"), icon_set()).with_title("   ");
    assert!(blank_title.validate().is_err());
    let blank_explicit = spec("app.status").with_accessibility_label(" \t ");
    assert!(blank_explicit.validate().is_err());
}

#[test]
fn text_fields_are_bounded_utf8_without_control_characters() {
    let long = "é".repeat(MAX_STATUS_TEXT_BYTES / 2 + 1);
    assert!(spec("app.status").with_title(long).validate().is_err());
    let max = "é".repeat(MAX_STATUS_TEXT_BYTES / 2);
    assert!(spec("app.status").with_title(max).validate().is_ok());
    assert!(
        spec("app.status")
            .with_title("bad\u{7}title")
            .validate()
            .is_err()
    );
    assert!(
        spec("app.status")
            .with_tooltip("bad\0tooltip")
            .validate()
            .is_err()
    );
    assert!(
        spec("app.status")
            .with_accessibility_label("bad\u{7f}label")
            .validate()
            .is_err()
    );
}

#[test]
fn asset_keys_are_portable_relative_identifiers() {
    for value in ["tray.png", "icons/tray.png", "icons/dark/2x.png", "a-b_c.1"] {
        assert!(
            IconPayload::asset_key(value).is_ok(),
            "{value:?} must be accepted"
        );
    }
    for value in [
        "",
        "/absolute/tray.png",
        "\\absolute\\tray.png",
        "icons\\tray.png",
        "C:/tray.png",
        "C:tray.png",
        "icons/./tray.png",
        "icons/../tray.png",
        "icons//tray.png",
        "icons/",
        "bad\0path",
        "bad\npath",
        "scheme:asset",
    ] {
        assert!(
            IconPayload::asset_key(value).is_err(),
            "{value:?} must be rejected"
        );
    }
    assert!(IconPayload::asset_key("a".repeat(MAX_ICON_ASSET_BYTES)).is_ok());
    assert!(IconPayload::asset_key("a".repeat(MAX_ICON_ASSET_BYTES + 1)).is_err());
}

#[test]
fn rgba_payloads_match_dimensions_and_limits() {
    assert!(IconPayload::rgba(1, 1, vec![0; 4]).is_ok());
    assert!(IconPayload::rgba(16, 16, vec![0; 1024]).is_ok());
    assert!(IconPayload::rgba(0, 1, vec![0; 4]).is_err());
    assert!(IconPayload::rgba(1, 0, vec![0; 4]).is_err());
    assert!(IconPayload::rgba(MAX_ICON_DIMENSION + 1, 1, vec![0; 1028]).is_err());
    assert!(IconPayload::rgba(1, 1, vec![0; 3]).is_err());
    assert!(IconPayload::rgba(2, 2, vec![0; 15]).is_err());
    assert!(IconPayload::rgba(u32::MAX, 2, vec![0; 8]).is_err());
}

#[test]
fn icon_sets_require_at_least_one_valid_variant() {
    assert!(
        StatusIconSet::new(std::iter::empty()).is_err(),
        "an empty icon set is not a status icon"
    );
    let invalid = StatusIconVariant::new(
        IconPayload::Rgba {
            width: 2,
            height: 2,
            bytes: vec![0; 8],
        },
        ColorSchemeMatch::Any,
        ScaleBucket::X1,
    );
    assert!(StatusIconSet::new([invalid]).is_err());
}

#[test]
fn selection_prefers_contrast_then_scheme_then_scale() {
    let hc = icon_variant("hc", ColorSchemeMatch::Any, ScaleBucket::X1)
        .with_high_contrast(HighContrastMatch::HighContrastOnly);
    let light = icon_variant("light", ColorSchemeMatch::Light, ScaleBucket::X1);
    let dark = icon_variant("dark", ColorSchemeMatch::Dark, ScaleBucket::X1);
    let set = StatusIconSet::new([hc.clone(), light.clone(), dark.clone()]).expect("set");

    let preferences = SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        high_contrast: PreferenceValue::Known(true),
        ..SystemPreferences::default()
    };
    assert_eq!(set.select_best(&preferences, 1000), &hc);

    let preferences = SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        high_contrast: PreferenceValue::Known(false),
        ..SystemPreferences::default()
    };
    assert_eq!(set.select_best(&preferences, 1000), &dark);

    let preferences = SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Light),
        high_contrast: PreferenceValue::Known(false),
        ..SystemPreferences::default()
    };
    assert_eq!(set.select_best(&preferences, 1000), &light);

    let unavailable = SystemPreferences::default();
    assert_eq!(set.select_best(&unavailable, 1000), &light);
}

#[test]
fn selection_downscales_instead_of_upscaling_blurry_icons() {
    let x1 = icon_variant("x1", ColorSchemeMatch::Any, ScaleBucket::X1);
    let x125 = icon_variant("x125", ColorSchemeMatch::Any, ScaleBucket::X1_25);
    let x15 = icon_variant("x15", ColorSchemeMatch::Any, ScaleBucket::X1_5);
    let x2 = icon_variant("x2", ColorSchemeMatch::Any, ScaleBucket::X2);
    let x3 = icon_variant("x3", ColorSchemeMatch::Any, ScaleBucket::X3);
    let set = StatusIconSet::new([
        x1.clone(),
        x125.clone(),
        x15.clone(),
        x2.clone(),
        x3.clone(),
    ])
    .expect("set");

    for (requested, expected) in [
        (0, &x1),
        (1, &x1),
        (1000, &x1),
        (1001, &x125),
        (1250, &x125),
        (1300, &x15),
        (2000, &x2),
        (2500, &x3),
        (3000, &x3),
        (4000, &x3),
    ] {
        assert_eq!(
            set.select_best(&SystemPreferences::default(), requested),
            expected,
            "requested milli-scale {requested}"
        );
    }
}

#[test]
fn selection_ties_are_resolved_by_declaration_order() {
    let first = icon_variant("first", ColorSchemeMatch::Any, ScaleBucket::X1);
    let second = icon_variant("second", ColorSchemeMatch::Any, ScaleBucket::X1);
    let set = StatusIconSet::new([first.clone(), second]).expect("set");
    assert_eq!(set.select_best(&SystemPreferences::default(), 1000), &first);
}

#[test]
fn template_metadata_is_preserved_by_selection() {
    let template =
        icon_variant("template", ColorSchemeMatch::Any, ScaleBucket::X2).with_template(true);
    let set = StatusIconSet::single(template.clone()).expect("set");
    let selected = set.select_best(&SystemPreferences::default(), 2000);
    assert!(selected.is_template);
    assert_eq!(selected, &template);
}

#[test]
fn mobile_alternatives_validate_identifiers() {
    assert!(MobileAlternativeSurface::ongoing_notification("channel", "notification").is_ok());
    assert!(MobileAlternativeSurface::shortcut("tray.shortcut").is_ok());
    assert!(MobileAlternativeSurface::ongoing_notification("", "notification").is_err());
    assert!(MobileAlternativeSurface::ongoing_notification("channel", " ").is_err());
    assert!(MobileAlternativeSurface::shortcut("bad\0shortcut").is_err());
    assert!(
        MobileAlternativeSurface::shortcut("a".repeat(MAX_ALTERNATIVE_IDENTIFIER_BYTES + 1))
            .is_err()
    );
    assert!(MobileAlternativeSurface::None.validate().is_ok());
    assert!(
        MobileAlternativeSurface::InAppStatusStrip
            .validate()
            .is_ok()
    );
}

#[test]
fn click_actions_validate_system_action_identity() {
    assert!(TrayClickAction::dispatch_system_action("app.quit").is_ok());
    assert!(TrayClickAction::dispatch_system_action("").is_err());
    assert!(TrayClickAction::dispatch_system_action("bad action").is_err());
    let action = TrayClickAction::invoke_command(command_id("app.quit"));
    assert_eq!(action.command_id(), Some(&command_id("app.quit")));
    assert_eq!(action.action_id(), None);
}

#[test]
fn residency_policy_helpers_recognize_commands_and_actions() {
    let mut policy = TrayResidencyPolicy::new();
    assert!(policy.quit_command_ids.is_empty());
    assert!(policy.register_quit_command(command_id("app.quit")));
    assert!(policy.register_restore_command(command_id("app.restore")));
    policy
        .register_quit_system_action("app.system.quit")
        .expect("valid action");
    policy
        .register_restore_system_action("app.system.restore")
        .expect("valid action");
    assert!(policy.is_quit_command(&command_id("app.quit")));
    assert!(policy.is_restore_command(&command_id("app.restore")));
    assert!(policy.is_quit_system_action("app.system.quit"));
    assert!(policy.is_restore_system_action("app.system.restore"));
    assert!(policy.register_quit_system_action("bad action").is_err());
}
