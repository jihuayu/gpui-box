//! Android platform integration. Android's main Looper owns the GPUI runtime.
//!
//! The platform uses JNI and NDK handles at its boundary. Distribution consumers
//! do not need unsafe code. See platforms/android for the matching Kotlin host.

#[cfg(any(target_os = "android", test))]
mod lifecycle;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::*;
