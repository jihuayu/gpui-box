//! Generic code that is shared between multiple layout algorithms
pub(crate) mod alignment;
pub(crate) mod intrinsic_limits;
pub(crate) mod size_containment;
pub(crate) mod sizing_keyword;

#[cfg(feature = "content_size")]
pub(crate) mod scrollable_overflow;
