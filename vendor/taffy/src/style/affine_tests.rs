use super::*;
use crate::util::MaybeResolve;

#[test]
fn owned_affine_resolution_retains_basis_and_property_range() {
    let signed = LengthPercentageAuto::affine(-20.0, 0.1);
    assert_eq!(signed.maybe_resolve(None, |_, _| panic!()), None);
    assert_eq!(
        signed.maybe_resolve(Some(100.0), |_, _| panic!()),
        Some(-10.0)
    );
    assert_eq!(
        Dimension::affine(-20.0, 0.1).maybe_resolve(Some(100.0), |_, _| panic!()),
        Some(0.0)
    );
    assert_eq!(
        LengthPercentage::affine(-20.0, 0.1).maybe_resolve(Some(100.0), |_, _| panic!()),
        Some(0.0)
    );
    assert_eq!(
        Dimension::affine(30.0, 0.0).maybe_resolve(None, |_, _| panic!()),
        None
    );
    let v = CompactLength::affine(-12.0, 0.5);
    assert!(v.uses_percentage());
    assert!(v.is_length_or_percentage());
    assert!(!v.has_calc());
    assert!(!v.is_sizing_keyword());
    assert!(!v.is_fit_content());
    let fit = CompactLength::fit_content_affine(-12.0, 0.5);
    assert!(fit.is_sizing_keyword());
    assert!(fit.is_fit_content());
    assert!(fit.is_intrinsic());
    assert!(fit.uses_percentage());
    assert!(!fit.has_calc());
    assert!(!fit.is_fit_content_calc());
    assert_eq!(
        Dimension::from(Dimension::affine(-12.0, 0.5).expand()),
        Dimension::affine(-12.0, 0.5)
    );
    assert_eq!(
        Dimension::from(Dimension::fit_content_affine(-12.0, 0.5).expand()),
        Dimension::fit_content_affine(-12.0, 0.5)
    );
}

#[cfg(feature = "grid")]
#[test]
fn owned_affine_grid_limits_and_domain_conversions() {
    let track = MaxTrackSizingFunction::affine(-20.0, 0.5);
    assert!(track.has_definite_value(Some(100.0)));
    assert!(!track.has_definite_value(None));
    assert_eq!(
        track.definite_value(Some(100.0), |_, _| panic!()),
        Some(30.0)
    );
    assert_eq!(track.definite_value(Some(10.0), |_, _| panic!()), Some(0.0));
    let fit = MaxTrackSizingFunction::fit_content_affine(-20.0, 0.5);
    assert_eq!(
        fit.fit_content_limit(Some(100.0), |_, _| panic!()),
        Some(30.0)
    );
    assert_eq!(fit.definite_limit(None, |_, _| panic!()), None);
    assert_eq!(
        MinTrackSizingFunction::from(Dimension::fit_content_affine(10.0, 0.5)),
        MinTrackSizingFunction::auto()
    );
    assert_eq!(MaxTrackSizingFunction::from(fit.expand()), fit);
}

#[cfg(feature = "serde")]
#[test]
fn owned_affine_serialization_preserves_plain_format_and_values() {
    for value in [
        Dimension::length(2.0),
        Dimension::affine(-12.0, 0.5),
        Dimension::fit_content_affine(5.0, -0.1),
    ] {
        let text = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<Dimension>(&text).unwrap(), value);
    }
    assert!(
        !serde_json::to_string(&Dimension::length(2.0))
            .unwrap()
            .starts_with('[')
    );
    assert!(
        serde_json::to_string(&Dimension::affine(2.0, 0.0))
            .unwrap()
            .starts_with('[')
    );
}
