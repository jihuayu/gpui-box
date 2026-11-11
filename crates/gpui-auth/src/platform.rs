//! Six-platform authentication capability matrix.
//!
//! Every cell is [`Evidence::ContractOnly`]: this crate compiles contracts and
//! deterministic fakes. Native providers are tracked by #8 (desktop), #9 (iOS),
//! #70 (Web/Passkeys) and #74 (Android Passkey/Credential Manager).

use gpui_system::{Evidence, Platform};
use std::fmt;

/// The six deployment targets covered by the account-auth contract. The shared
/// [`Platform`] vocabulary has five native targets; Web is distribution-local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthPlatform {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

impl AuthPlatform {
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Android,
        Self::Ios,
        Self::Web,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Web => "web",
        }
    }

    pub const fn native_platform(self) -> Option<Platform> {
        match self {
            Self::Windows => Some(Platform::Windows),
            Self::MacOs => Some(Platform::MacOs),
            Self::Linux => Some(Platform::Linux),
            Self::Android => Some(Platform::Android),
            Self::Ios => Some(Platform::Ios),
            Self::Web => None,
        }
    }
}

impl fmt::Display for AuthPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Authentication operations with distinct platform availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthOperation {
    BrowserOAuthSession,
    EphemeralBrowserSession,
    LoopbackRedirect,
    CustomSchemeRedirect,
    UniversalLinkRedirect,
    WebSameOriginRedirect,
    PasskeyRegistration,
    PasskeyAssertion,
    PasskeyConditionalMediation,
    TokenVaultPersistence,
}

impl AuthOperation {
    pub const ALL: [Self; 10] = [
        Self::BrowserOAuthSession,
        Self::EphemeralBrowserSession,
        Self::LoopbackRedirect,
        Self::CustomSchemeRedirect,
        Self::UniversalLinkRedirect,
        Self::WebSameOriginRedirect,
        Self::PasskeyRegistration,
        Self::PasskeyAssertion,
        Self::PasskeyConditionalMediation,
        Self::TokenVaultPersistence,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BrowserOAuthSession => "browser_oauth_session",
            Self::EphemeralBrowserSession => "ephemeral_browser_session",
            Self::LoopbackRedirect => "loopback_redirect",
            Self::CustomSchemeRedirect => "custom_scheme_redirect",
            Self::UniversalLinkRedirect => "universal_link_redirect",
            Self::WebSameOriginRedirect => "web_same_origin_redirect",
            Self::PasskeyRegistration => "passkey_registration",
            Self::PasskeyAssertion => "passkey_assertion",
            Self::PasskeyConditionalMediation => "passkey_conditional_mediation",
            Self::TokenVaultPersistence => "token_vault_persistence",
        }
    }
}

impl fmt::Display for AuthOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Three-state support classification used by the auth matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SupportLevel {
    Supported,
    Conditional,
    Unsupported,
}

impl SupportLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Conditional => "conditional",
            Self::Unsupported => "unsupported",
        }
    }
}

/// One contract-only matrix cell with the authentic native API and its
/// configuration prerequisites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformOperationSupport {
    pub platform: AuthPlatform,
    pub operation: AuthOperation,
    pub support: SupportLevel,
    /// Always [`Evidence::ContractOnly`] in this crate.
    pub evidence: Evidence,
    pub native_api: &'static str,
    pub prerequisites: &'static str,
}

/// Capability manifest for one platform. No native adapter is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthPlatformManifest {
    pub platform: AuthPlatform,
    pub native_adapter_installed: bool,
    pub evidence: Evidence,
}

pub const AUTH_PLATFORM_MANIFEST: [AuthPlatformManifest; 6] = [
    AuthPlatformManifest {
        platform: AuthPlatform::Windows,
        native_adapter_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AuthPlatformManifest {
        platform: AuthPlatform::MacOs,
        native_adapter_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AuthPlatformManifest {
        platform: AuthPlatform::Linux,
        native_adapter_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AuthPlatformManifest {
        platform: AuthPlatform::Android,
        native_adapter_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AuthPlatformManifest {
        platform: AuthPlatform::Ios,
        native_adapter_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AuthPlatformManifest {
        platform: AuthPlatform::Web,
        native_adapter_installed: false,
        evidence: Evidence::ContractOnly,
    },
];

pub const fn support(platform: AuthPlatform, operation: AuthOperation) -> SupportLevel {
    use AuthOperation as Op;
    use AuthPlatform as P;
    use SupportLevel as Level;
    match (platform, operation) {
        (
            P::Windows | P::MacOs | P::Linux | P::Android | P::Ios | P::Web,
            Op::BrowserOAuthSession,
        ) => Level::Supported,
        (P::Windows | P::MacOs | P::Ios, Op::EphemeralBrowserSession) => Level::Supported,
        (P::Linux | P::Android | P::Web, Op::EphemeralBrowserSession) => Level::Conditional,
        (P::Windows | P::MacOs | P::Linux, Op::LoopbackRedirect) => Level::Supported,
        (P::Android | P::Ios | P::Web, Op::LoopbackRedirect) => Level::Unsupported,
        (P::Windows | P::MacOs | P::Linux | P::Android | P::Ios, Op::CustomSchemeRedirect) => {
            Level::Supported
        }
        (P::Web, Op::CustomSchemeRedirect) => Level::Unsupported,
        (P::Android | P::Ios, Op::UniversalLinkRedirect) => Level::Supported,
        (P::Windows | P::MacOs | P::Linux | P::Web, Op::UniversalLinkRedirect) => {
            Level::Unsupported
        }
        (P::Web, Op::WebSameOriginRedirect) => Level::Supported,
        (P::Windows | P::MacOs | P::Linux | P::Android | P::Ios, Op::WebSameOriginRedirect) => {
            Level::Unsupported
        }
        (
            P::Windows | P::MacOs | P::Android | P::Ios,
            Op::PasskeyRegistration | Op::PasskeyAssertion,
        ) => Level::Supported,
        (P::Linux | P::Web, Op::PasskeyRegistration | Op::PasskeyAssertion) => Level::Conditional,
        (_, Op::PasskeyConditionalMediation) => Level::Conditional,
        (P::Windows | P::MacOs | P::Android | P::Ios, Op::TokenVaultPersistence) => {
            Level::Supported
        }
        (P::Linux | P::Web, Op::TokenVaultPersistence) => Level::Conditional,
    }
}

pub const fn native_api(platform: AuthPlatform, operation: AuthOperation) -> &'static str {
    use AuthOperation as Op;
    use AuthPlatform as P;
    match (platform, operation) {
        (P::Windows, Op::BrowserOAuthSession | Op::EphemeralBrowserSession) => {
            "WebView2 (custom profile for ephemeral) or ShellExecuteEx with an RFC 8252 external user-agent"
        }
        (P::Windows, Op::LoopbackRedirect) => {
            "WinSock2 loopback listener plus ShellExecuteEx (RFC 8252 §7.3)"
        }
        (P::Windows, Op::CustomSchemeRedirect) => {
            "Registered URL protocol handler and Win32 single-instance activation"
        }
        (P::Windows, Op::UniversalLinkRedirect | Op::WebSameOriginRedirect) => {
            "Not applicable on Windows"
        }
        (P::Windows, Op::PasskeyRegistration | Op::PasskeyAssertion) => {
            "WebAuthn API (webauthn.dll) with Windows Hello platform authenticator"
        }
        (P::Windows, Op::PasskeyConditionalMediation) => {
            "WebAuthn conditional mediation via a WebView2 credential UI"
        }
        (P::Windows, Op::TokenVaultPersistence) => {
            "Windows Credential Manager (CredWriteW/CredReadW) with per-user DPAPI protection"
        }
        (P::MacOs, Op::BrowserOAuthSession | Op::EphemeralBrowserSession) => {
            "ASWebAuthenticationSession (prefersEphemeralWebBrowserSession for ephemeral)"
        }
        (P::MacOs, Op::LoopbackRedirect) => {
            "Network.framework loopback listener plus NSWorkspace external user-agent"
        }
        (P::MacOs, Op::CustomSchemeRedirect) => {
            "CFBundleURLTypes handler and NSAppleEventManager URL delivery"
        }
        (P::MacOs, Op::UniversalLinkRedirect | Op::WebSameOriginRedirect) => {
            "Not applicable on macOS"
        }
        (P::MacOs, Op::PasskeyRegistration | Op::PasskeyAssertion) => {
            "AuthenticationServices ASAuthorizationPlatformPublicKeyCredentialProvider"
        }
        (P::MacOs, Op::PasskeyConditionalMediation) => {
            "AuthenticationServices conditional credential UI"
        }
        (P::MacOs, Op::TokenVaultPersistence) => {
            "Keychain Services with kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly"
        }
        (P::Linux, Op::BrowserOAuthSession) => {
            "xdg-open external user-agent with an RFC 8252 authorization request"
        }
        (P::Linux, Op::EphemeralBrowserSession) => {
            "Browser-specific private profile through the desktop portal; not guaranteed"
        }
        (P::Linux, Op::LoopbackRedirect) => "std loopback listener plus xdg-open (RFC 8252 §7.3)",
        (P::Linux, Op::CustomSchemeRedirect) => {
            "XDG desktop entry with MimeType=x-scheme-handler/<scheme>"
        }
        (P::Linux, Op::UniversalLinkRedirect | Op::WebSameOriginRedirect) => {
            "Not applicable on Linux"
        }
        (P::Linux, Op::PasskeyRegistration | Op::PasskeyAssertion) => {
            "libfido2 or the org.freedesktop.portal.Fido2 desktop portal"
        }
        (P::Linux, Op::PasskeyConditionalMediation) => {
            "Desktop portal/FIDO2 conditional UI; requires compositor and portal support"
        }
        (P::Linux, Op::TokenVaultPersistence) => {
            "Secret Service (libsecret) with a locked-keyring error path"
        }
        (P::Android, Op::BrowserOAuthSession | Op::EphemeralBrowserSession) => {
            "Android Custom Tabs via androidx.browser (ephemeral requires an isolated profile policy)"
        }
        (P::Android, Op::LoopbackRedirect | Op::WebSameOriginRedirect) => {
            "Not available to a native Android application"
        }
        (P::Android, Op::CustomSchemeRedirect) => {
            "Intent filter with a verified custom scheme and singleTop launch mode"
        }
        (P::Android, Op::UniversalLinkRedirect) => {
            "Android App Links with android:autoVerify and assetlinks.json"
        }
        (P::Android, Op::PasskeyRegistration | Op::PasskeyAssertion) => {
            "androidx.credentials Credential Manager with Google Password Manager/BIOS"
        }
        (P::Android, Op::PasskeyConditionalMediation) => {
            "Credential Manager conditional UI in a Custom Tab or WebView"
        }
        (P::Android, Op::TokenVaultPersistence) => {
            "Android Keystore-backed encryption with EncryptedSharedPreferences/DataStore"
        }
        (P::Ios, Op::BrowserOAuthSession | Op::EphemeralBrowserSession) => {
            "ASWebAuthenticationSession (prefersEphemeralWebBrowserSession for ephemeral)"
        }
        (P::Ios, Op::LoopbackRedirect | Op::WebSameOriginRedirect) => {
            "Not available to a native iOS application"
        }
        (P::Ios, Op::CustomSchemeRedirect) => {
            "CFBundleURLTypes handler delivered through scene openURLContexts"
        }
        (P::Ios, Op::UniversalLinkRedirect) => {
            "Universal Links with apple-app-site-association and associated-domains entitlement"
        }
        (P::Ios, Op::PasskeyRegistration | Op::PasskeyAssertion) => {
            "AuthenticationServices ASAuthorizationPlatformPublicKeyCredentialProvider"
        }
        (P::Ios, Op::PasskeyConditionalMediation) => {
            "AuthenticationServices conditional credential UI"
        }
        (P::Ios, Op::TokenVaultPersistence) => {
            "Keychain Services with kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly"
        }
        (P::Web, Op::BrowserOAuthSession | Op::EphemeralBrowserSession) => {
            "window.open/fetch with SameSite cookies or an in-memory ephemeral session"
        }
        (P::Web, Op::LoopbackRedirect | Op::CustomSchemeRedirect | Op::UniversalLinkRedirect) => {
            "Not available to a browser-origin application"
        }
        (P::Web, Op::WebSameOriginRedirect) => {
            "Same-origin redirect URI validated against window.location.origin"
        }
        (P::Web, Op::PasskeyRegistration | Op::PasskeyAssertion) => {
            "navigator.credentials.create/get in a secure context"
        }
        (P::Web, Op::PasskeyConditionalMediation) => {
            "navigator.credentials.get({ mediation: \"conditional\" }) behind PublicKeyCredential.isConditionalMediationAvailable()"
        }
        (P::Web, Op::TokenVaultPersistence) => {
            "Non-extractable WebCrypto keys plus origin-scoped IndexedDB; no plaintext token storage"
        }
    }
}

pub const fn prerequisites(platform: AuthPlatform, operation: AuthOperation) -> &'static str {
    use AuthOperation as Op;
    use AuthPlatform as P;
    match (platform, operation) {
        (_, Op::PasskeyRegistration | Op::PasskeyAssertion | Op::PasskeyConditionalMediation) => {
            "Secure context (https or loopback), RP ID/origin match and user-verification policy"
        }
        (P::Windows, _) => "Signed Win32 app, WebView2 runtime and per-user credential scope",
        (P::MacOs, _) => "Signed/notarized app with associated-domains or URL-type entitlements",
        (P::Linux, _) => "XDG desktop integration, portal and Secret Service availability",
        (P::Android, _) => {
            "Target SDK with verified App Links/assetlinks.json and Play Services for Credential Manager"
        }
        (P::Ios, _) => "Associated-domains entitlement, AASA file and Keychain access group",
        (P::Web, _) => {
            "Secure context, origin-scoped storage and no third-party cookie assumptions"
        }
    }
}

pub fn operation_support(operation: AuthOperation) -> Vec<PlatformOperationSupport> {
    AuthPlatform::ALL
        .into_iter()
        .map(|platform| PlatformOperationSupport {
            platform,
            operation,
            support: support(platform, operation),
            evidence: Evidence::ContractOnly,
            native_api: native_api(platform, operation),
            prerequisites: prerequisites(platform, operation),
        })
        .collect()
}

pub fn platform_operations(platform: AuthPlatform) -> Vec<PlatformOperationSupport> {
    AuthOperation::ALL
        .into_iter()
        .map(|operation| PlatformOperationSupport {
            platform,
            operation,
            support: support(platform, operation),
            evidence: Evidence::ContractOnly,
            native_api: native_api(platform, operation),
            prerequisites: prerequisites(platform, operation),
        })
        .collect()
}

/// Sixty unique cells, all contract-only.
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    AuthPlatform::ALL
        .into_iter()
        .flat_map(platform_operations)
        .collect()
}
