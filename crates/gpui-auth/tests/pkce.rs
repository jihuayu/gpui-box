//! PKCE, SHA-256, base64url, URL and relying-party validation.

mod support;

use gpui_auth::*;

fn hex(bytes: &[u8]) -> String {
    let mut output = String::new();
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[test]
fn sha256_matches_fips_180_4_vectors() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    // Message spanning the 55/56-byte padding boundary.
    assert_eq!(
        hex(&sha256(
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno"
        ))
        .len(),
        64
    );
}

#[test]
fn base64url_matches_rfc_4648_without_padding() {
    assert_eq!(base64url_encode(b""), "");
    assert_eq!(base64url_encode(b"f"), "Zg");
    assert_eq!(base64url_encode(b"fo"), "Zm8");
    assert_eq!(base64url_encode(b"foo"), "Zm9v");
    assert_eq!(base64url_encode(b"foob"), "Zm9vYg");
    assert_eq!(base64url_encode(b"fooba"), "Zm9vYmE");
    assert_eq!(base64url_encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
}

#[test]
fn rfc_7636_appendix_b_s256_vector_is_exact() {
    let verifier =
        PkceVerifier::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").expect("verifier");
    assert_eq!(
        verifier.challenge().as_str(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(format!("{verifier:?}"), "PkceVerifier([REDACTED])");
    let pair = verifier.into_pair();
    assert_eq!(pair.challenge().as_str().len(), PKCE_CHALLENGE_LEN);
    assert!(format!("{pair:?}").contains("REDACTED"));
    assert!(!format!("{pair:?}").contains("dBjftJeZ"));
}

#[test]
fn verifier_bounds_and_characters_are_enforced() {
    assert!(PkceVerifier::new("a".repeat(MIN_PKCE_VERIFIER_LEN)).is_ok());
    assert!(PkceVerifier::new("a".repeat(MAX_PKCE_VERIFIER_LEN)).is_ok());
    assert!(PkceVerifier::new("a".repeat(MIN_PKCE_VERIFIER_LEN - 1)).is_err());
    assert!(PkceVerifier::new("a".repeat(MAX_PKCE_VERIFIER_LEN + 1)).is_err());
    assert!(PkceVerifier::new(format!("{}!", "a".repeat(42))).is_err());
    assert!(PkceVerifier::new(format!("{} ", "a".repeat(42))).is_err());
}

#[test]
fn endpoint_and_origin_validation_is_strict() {
    assert!(AuthEndpointUrl::parse("https://idp.example/authorize").is_ok());
    assert!(AuthEndpointUrl::parse("http://127.0.0.1:8080/authorize").is_err());
    assert!(AuthEndpointUrl::parse_with_loopback("http://127.0.0.1:8080/authorize", true).is_ok());
    assert!(AuthEndpointUrl::parse("https://user:pass@idp.example/authorize").is_err());
    assert!(AuthEndpointUrl::parse("https://idp.example/authorize#fragment").is_err());
    assert!(AuthEndpointUrl::parse("https://idp.example:0/authorize").is_err());
    assert_eq!(
        AuthEndpointUrl::parse("https://idp.example:/authorize"),
        Err(UrlError::InvalidPort)
    );
    assert!(AuthEndpointUrl::parse("https:///authorize").is_err());
    assert!(AuthEndpointUrl::parse("https://idp.example/a b").is_err());

    let origin = AuthOrigin::parse("https://login.example.com").expect("origin");
    assert!(origin.is_secure_context());
    assert!(AuthOrigin::parse("http://localhost:3000").is_ok());
    assert!(AuthOrigin::parse("http://[::1]:3000").is_ok());
    assert!(AuthOrigin::parse("http://example.com").is_err());
    assert!(AuthOrigin::parse("https://example.com/path").is_err());
    assert!(AuthOrigin::parse("https://example.com?q=1").is_err());
}

#[test]
fn relying_party_effective_domain_matching_is_label_safe() {
    assert!(validate_rp_id("example.com"));
    assert!(validate_rp_id("auth.example.com"));
    assert!(!validate_rp_id("Example.com"));
    assert!(!validate_rp_id("127.0.0.1"));
    assert!(!validate_rp_id("[::1]"));
    assert!(!validate_rp_id("example.com."));
    assert!(!validate_rp_id("-example.com"));
    assert!(!validate_rp_id("example..com"));
    assert!(!validate_rp_id("example.com:443"));

    let rp = RelyingPartyId::new("example.com").expect("rp");
    let origin = AuthOrigin::parse("https://login.example.com").expect("origin");
    assert!(rp.matches_origin(&origin));
    let origin = AuthOrigin::parse("https://example.com").expect("origin");
    assert!(rp.matches_origin(&origin));
    let origin = AuthOrigin::parse("https://badexample.com").expect("origin");
    assert!(!rp.matches_origin(&origin));
    let origin = AuthOrigin::parse("http://localhost:3000").expect("origin");
    assert!(!rp.matches_origin(&origin));
}

#[test]
fn redirect_targets_build_exact_uris_and_match_callbacks() {
    let loopback = RedirectTarget::loopback(LoopbackHost::Ipv4, Some(49_152), "/oauth/callback")
        .expect("loopback");
    assert_eq!(
        loopback.redirect_uri().expect("uri"),
        "http://127.0.0.1:49152/oauth/callback"
    );
    assert!(loopback.is_loopback());
    let unresolved =
        RedirectTarget::loopback(LoopbackHost::Ipv6, None, "/oauth/callback").expect("loopback");
    assert!(unresolved.redirect_uri().is_err());
    let resolved = unresolved.resolve_loopback_port(49_153).expect("resolve");
    assert_eq!(
        resolved.redirect_uri().expect("uri"),
        "http://[::1]:49153/oauth/callback"
    );

    let custom = RedirectTarget::custom_scheme("com.example.app", Some("auth"), "/callback")
        .expect("custom");
    assert_eq!(
        custom.redirect_uri().expect("uri"),
        "com.example.app://auth/callback"
    );
    assert!(RedirectTarget::custom_scheme("https", Some("auth"), "/callback").is_err());
    assert!(RedirectTarget::custom_scheme("javascript", None, "/x").is_err());
    assert!(RedirectTarget::custom_scheme("com.example.app", None, "/x").is_ok());

    let link = RedirectTarget::universal_link("links.example.com", None, "/auth/cb")
        .expect("universal link");
    assert_eq!(
        link.redirect_uri().expect("uri"),
        "https://links.example.com/auth/cb"
    );
    assert!(RedirectTarget::universal_link("127.0.0.1", None, "/cb").is_err());
    assert!(RedirectTarget::universal_link("localhost", None, "/cb").is_err());

    let origin = AuthOrigin::parse("https://app.example.com").expect("origin");
    let web = RedirectTarget::web_same_origin(origin, "/auth/callback").expect("web");
    assert_eq!(
        web.redirect_uri().expect("uri"),
        "https://app.example.com/auth/callback"
    );
    assert!(web.is_web());
}

#[test]
fn callback_matching_is_exact_and_fragment_free() {
    let target = RedirectTarget::loopback(LoopbackHost::Ipv4, Some(49_152), "/oauth/callback")
        .expect("loopback");
    let callback =
        CallbackUrl::parse("http://127.0.0.1:49152/oauth/callback?code=x&state=y").expect("cb");
    assert!(target.matches_callback(&callback));
    assert!(!format!("{callback:?}").contains("code"));
    assert!(!format!("{callback:?}").contains("state"));
    let wrong_port =
        CallbackUrl::parse("http://127.0.0.1:49153/oauth/callback?state=y").expect("cb");
    assert!(!target.matches_callback(&wrong_port));
    let wrong_path = CallbackUrl::parse("http://127.0.0.1:49152/other?state=y").expect("cb");
    assert!(!target.matches_callback(&wrong_path));
    assert!(CallbackUrl::parse("http://127.0.0.1:49152/cb#access_token=x").is_err());

    let ipv6 = RedirectTarget::loopback(LoopbackHost::Ipv6, Some(49_152), "/cb").expect("loopback");
    let callback = CallbackUrl::parse("http://[::1]:49152/cb?state=y").expect("cb");
    assert!(ipv6.matches_callback(&callback));

    let custom =
        RedirectTarget::custom_scheme("com.example.app", Some("auth"), "/cb").expect("custom");
    let callback = CallbackUrl::parse("com.example.app://auth/cb?state=y").expect("cb");
    assert!(custom.matches_callback(&callback));
    let callback = CallbackUrl::parse("com.example.app://evil/cb?state=y").expect("cb");
    assert!(!custom.matches_callback(&callback));

    let link = RedirectTarget::universal_link("links.example.com", None, "/cb").expect("link");
    let callback = CallbackUrl::parse("https://links.example.com/cb?state=y").expect("cb");
    assert!(link.matches_callback(&callback));
    let callback = CallbackUrl::parse("https://links.example.com:8443/cb?state=y").expect("cb");
    assert!(!link.matches_callback(&callback));

    let origin = AuthOrigin::parse("https://app.example.com").expect("origin");
    let web = RedirectTarget::web_same_origin(origin, "/cb").expect("web");
    let callback = CallbackUrl::parse("https://app.example.com/cb?state=y").expect("cb");
    assert!(web.matches_callback(&callback));
    let callback = CallbackUrl::parse("https://other.example.com/cb?state=y").expect("cb");
    assert!(!web.matches_callback(&callback));
}

#[test]
fn oauth_error_codes_are_sanitized_and_typed() {
    assert_eq!(
        OAuthErrorCode::parse("access_denied"),
        OAuthErrorCode::AccessDenied
    );
    assert!(OAuthErrorCode::AccessDenied.is_user_cancellation());
    assert_eq!(
        OAuthErrorCode::parse("invalid_request"),
        OAuthErrorCode::InvalidRequest
    );
    assert_eq!(
        OAuthErrorCode::parse("temporarily_unavailable"),
        OAuthErrorCode::TemporarilyUnavailable
    );
    assert_eq!(
        OAuthErrorCode::parse("private description text"),
        OAuthErrorCode::Other
    );
}

#[test]
fn redirect_bounds_and_zero_ids_are_enforced() {
    assert!(RedirectTarget::loopback(LoopbackHost::Ipv4, Some(0), "/cb").is_err());
    assert!(RedirectTarget::loopback(LoopbackHost::Ipv4, Some(1), "cb").is_err());
    assert!(RedirectTarget::loopback(LoopbackHost::Ipv4, Some(1), "/cb?x=1").is_err());
    assert!(PresentationContext::new(gpui_system::SurfaceId(0), 1).is_err());
    assert!(PresentationContext::new(gpui_system::SurfaceId(1), 0).is_err());
    assert!(CeremonyId::new(0).is_err());
    let _ = support::state_for(1);
}
