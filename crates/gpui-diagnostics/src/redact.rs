//! Deterministic, allocation-bounded sensitive-data redaction.
//!
//! Redaction runs when a value is constructed or ingested. No unredacted
//! payload is retained in a public diagnostic type. The rules are intentionally
//! conservative: over-redacting a diagnostic string is preferable to leaking a
//! credential, session identifier or private filesystem location.

use std::fmt;

/// Marker prefix used by every redacted value.
pub const REDACTED_PREFIX: &str = "[REDACTED:";

/// The reason a value or field was redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RedactionKind {
    Token,
    Secret,
    Password,
    Authorization,
    Cookie,
    Credential,
    PrivateKey,
    ApiKey,
    Session,
    Path,
    Bearer,
    Basic,
    Jwt,
    UrlUserInfo,
    UrlQuery,
}

impl RedactionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Secret => "secret",
            Self::Password => "password",
            Self::Authorization => "authorization",
            Self::Cookie => "cookie",
            Self::Credential => "credential",
            Self::PrivateKey => "private_key",
            Self::ApiKey => "api_key",
            Self::Session => "session",
            Self::Path => "path",
            Self::Bearer => "bearer",
            Self::Basic => "basic",
            Self::Jwt => "jwt",
            Self::UrlUserInfo => "url_userinfo",
            Self::UrlQuery => "url_query",
        }
    }

    pub fn marker(self) -> String {
        format!("{REDACTED_PREFIX}{}]", self.as_str())
    }
}

impl fmt::Display for RedactionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Return the redaction marker for one kind.
pub fn redaction_marker(kind: RedactionKind) -> String {
    kind.marker()
}

/// Normalize an arbitrary field key into lowercase underscore-separated tokens.
///
/// Camel-case boundaries become underscores, punctuation collapses to a single
/// underscore, and leading/trailing separators are removed. This handles
/// `apiKey`, `x-api-key`, `file_path`, `Authorization` and similar spellings.
pub fn normalize_key(key: &str) -> String {
    let mut normalized = String::with_capacity(key.len() + 4);
    let mut previous_lower_or_digit = false;
    for character in key.chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase()
                && previous_lower_or_digit
                && !normalized.ends_with('_')
            {
                normalized.push('_');
            }
            normalized.push(character.to_ascii_lowercase());
            previous_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
        } else {
            if !normalized.is_empty() && !normalized.ends_with('_') {
                normalized.push('_');
            }
            previous_lower_or_digit = false;
        }
    }
    while normalized.ends_with('_') {
        normalized.pop();
    }
    normalized
}

/// Classify a field key. Keys are matched case-insensitively and across
/// normalized tokens/substrings, including camel-case and `x-api-key` forms.
pub fn sensitive_key_kind(key: &str) -> Option<RedactionKind> {
    let normalized = normalize_key(key);
    if normalized.is_empty() {
        return None;
    }
    let compact: String = normalized
        .chars()
        .filter(|character| *character != '_')
        .collect();
    let contains = |needle: &str| normalized.contains(needle);
    let compact_contains = |needle: &str| compact.contains(needle);

    if compact_contains("privatekey") || (contains("private") && contains("key")) {
        return Some(RedactionKind::PrivateKey);
    }
    if compact_contains("apikey") || normalized.contains("api_key") {
        return Some(RedactionKind::ApiKey);
    }
    if contains("authorization") || normalized == "auth" || normalized.ends_with("_auth") {
        return Some(RedactionKind::Authorization);
    }
    if contains("cookie") {
        return Some(RedactionKind::Cookie);
    }
    if contains("credential") {
        return Some(RedactionKind::Credential);
    }
    if contains("password") || contains("passwd") {
        return Some(RedactionKind::Password);
    }
    if contains("token") {
        return Some(RedactionKind::Token);
    }
    if contains("secret") {
        return Some(RedactionKind::Secret);
    }
    if contains("session") {
        return Some(RedactionKind::Session);
    }
    if contains("filepath") || contains("file_path") || contains("path") {
        return Some(RedactionKind::Path);
    }
    None
}

fn is_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '.' | '_' | '-' | '~' | '+' | '/' | '=' | '%' | ':' | '@'
        )
}

fn is_text_boundary(previous: Option<char>) -> bool {
    previous.is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
}

fn is_path_boundary(previous: Option<char>) -> bool {
    previous.is_none_or(|character| {
        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '/' | '\\' | ':' | '.')
    })
}

fn starts_with_ignore_ascii_case(input: &str, prefix: &str) -> bool {
    input
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

/// Replace sensitive string patterns with stable `[REDACTED:<kind>]` markers.
pub fn redact_text(input: &str) -> String {
    let urls = redact_urls(input);
    let headers = redact_auth_headers(&urls);
    let assignments = redact_sensitive_assignments(&headers);
    redact_paths(&assignments)
}

fn redact_urls(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        let rest = &input[index..];
        if starts_with_ignore_ascii_case(rest, "https://")
            || starts_with_ignore_ascii_case(rest, "http://")
        {
            let end = url_end(rest);
            let candidate = &rest[..end];
            output.push_str(&match url_sensitive_kind(candidate) {
                Some(kind) => kind.marker(),
                None => candidate.to_owned(),
            });
            index += end;
            continue;
        }
        let character = rest.chars().next().expect("nonempty remainder");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn url_end(input: &str) -> usize {
    input
        .char_indices()
        .find_map(|(index, character)| {
            (character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
                ))
            .then_some(index)
        })
        .unwrap_or(input.len())
}

fn url_sensitive_kind(url: &str) -> Option<RedactionKind> {
    let scheme_end = url.find("://")? + 3;
    let authority_end = url[scheme_end..]
        .find(['/', '?', '#'])
        .map(|offset| scheme_end + offset)
        .unwrap_or(url.len());
    let authority = &url[scheme_end..authority_end];
    if authority.contains('@') {
        return Some(RedactionKind::UrlUserInfo);
    }
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    for pair in query.split(['&', ';']) {
        let name = pair.split('=').next().unwrap_or_default();
        if sensitive_key_kind(name).is_some() {
            return Some(RedactionKind::UrlQuery);
        }
    }
    None
}

fn redact_sensitive_assignments(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        let rest = &input[index..];
        let character = rest.chars().next().expect("nonempty remainder");
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            let previous = input[..index].chars().next_back();
            if is_text_boundary(previous) {
                let key_end = rest
                    .char_indices()
                    .find_map(|(offset, candidate)| {
                        (!(candidate.is_ascii_alphanumeric()
                            || candidate == '_'
                            || candidate == '-'))
                            .then_some(offset)
                    })
                    .unwrap_or(rest.len());
                let key = &rest[..key_end];
                let after_key = &rest[key_end..];
                if let Some(after_equals) = after_key.strip_prefix('=')
                    && let Some(kind) = sensitive_key_kind(key)
                {
                    let value_len = after_equals
                        .char_indices()
                        .find_map(|(offset, candidate)| {
                            (candidate.is_whitespace()
                                || matches!(
                                    candidate,
                                    '"' | '\''
                                        | '`'
                                        | '<'
                                        | '>'
                                        | '('
                                        | ')'
                                        | '['
                                        | ']'
                                        | '{'
                                        | '}'
                                        | ','
                                        | ';'
                                        | '&'
                                        | '?'
                                ))
                            .then_some(offset)
                        })
                        .unwrap_or(after_equals.len());
                    if value_len > 0 {
                        output.push_str(key);
                        output.push('=');
                        output.push_str(&kind.marker());
                        index += key_end + 1 + value_len;
                        continue;
                    }
                }
            }
        }
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn redact_auth_headers(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        let rest = &input[index..];
        let previous = input[..index].chars().next_back();
        if is_text_boundary(previous) {
            if let Some((consumed, kind)) = auth_header(rest) {
                output.push_str(&kind.marker());
                index += consumed;
                continue;
            }
            if let Some(consumed) = jwt_token(rest) {
                output.push_str(&RedactionKind::Jwt.marker());
                index += consumed;
                continue;
            }
        }
        let character = rest.chars().next().expect("nonempty remainder");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn auth_header(input: &str) -> Option<(usize, RedactionKind)> {
    for (scheme, kind) in [
        ("bearer", RedactionKind::Bearer),
        ("basic", RedactionKind::Basic),
    ] {
        if !starts_with_ignore_ascii_case(input, scheme) {
            continue;
        }
        let after_scheme = &input[scheme.len()..];
        if !after_scheme.starts_with(|character: char| character.is_whitespace()) {
            continue;
        }
        let leading = after_scheme
            .char_indices()
            .find_map(|(offset, character)| (!character.is_whitespace()).then_some(offset))
            .unwrap_or(after_scheme.len());
        if leading == after_scheme.len() {
            continue;
        }
        let value = &after_scheme[leading..];
        let value_len = value
            .char_indices()
            .find_map(|(offset, character)| (!is_token_character(character)).then_some(offset))
            .unwrap_or(value.len());
        if value_len == 0 {
            continue;
        }
        return Some((scheme.len() + leading + value_len, kind));
    }
    None
}

fn jwt_token(input: &str) -> Option<usize> {
    if !input.starts_with("eyJ") {
        return None;
    }
    let end = input
        .char_indices()
        .find_map(|(offset, character)| {
            (!(character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '=')))
                .then_some(offset)
        })
        .unwrap_or(input.len());
    let candidate = &input[..end];
    let segments: Vec<&str> = candidate.split('.').collect();
    (segments.len() >= 3
        && segments[0].starts_with("eyJ")
        && segments.iter().take(3).all(|segment| !segment.is_empty()))
    .then_some(end)
}

fn redact_paths(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        let rest = &input[index..];
        let previous = input[..index].chars().next_back();
        if is_path_boundary(previous)
            && let Some((consumed, kind)) = path_token(rest)
        {
            output.push_str(&kind.marker());
            index += consumed;
            continue;
        }
        let character = rest.chars().next().expect("nonempty remainder");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn path_token(input: &str) -> Option<(usize, RedactionKind)> {
    let first = input.chars().next()?;
    if first == '/' && !input.starts_with("//") {
        let end = path_end(input);
        let candidate = trim_path_trailing(input, end);
        if is_unix_absolute_path(&candidate) {
            return Some((candidate.len(), RedactionKind::Path));
        }
        return None;
    }
    if first == '\\' && input.starts_with("\\\\") {
        let end = path_end(input);
        let candidate = trim_path_trailing(input, end);
        if is_windows_unc_path(&candidate) {
            return Some((candidate.len(), RedactionKind::Path));
        }
        return None;
    }
    if first.is_ascii_alphabetic() {
        let mut characters = input.chars();
        let _drive = characters.next();
        if characters.next() == Some(':')
            && characters
                .next()
                .is_some_and(|next| matches!(next, '\\' | '/'))
        {
            let end = path_end(input);
            let candidate = trim_path_trailing(input, end);
            if is_windows_drive_path(&candidate) {
                return Some((candidate.len(), RedactionKind::Path));
            }
        }
    }
    None
}

fn path_end(input: &str) -> usize {
    input
        .char_indices()
        .find_map(|(index, character)| {
            (character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
                ))
            .then_some(index)
        })
        .unwrap_or(input.len())
}

fn trim_path_trailing(input: &str, mut end: usize) -> String {
    let mut candidate = input[..end].to_owned();
    while candidate
        .chars()
        .next_back()
        .is_some_and(|character| matches!(character, '.' | ',' | ';' | ':' | '!' | '?'))
    {
        candidate.pop();
        end = candidate.len();
    }
    let _ = end;
    candidate
}

fn is_unix_absolute_path(path: &str) -> bool {
    if !path.starts_with('/') || path.starts_with("//") {
        return false;
    }
    let components: Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    const KNOWN_ROOTS: [&str; 10] = [
        "home", "Users", "var", "tmp", "etc", "opt", "private", "mnt", "proc", "root",
    ];
    if components.is_empty() {
        return false;
    }
    components.len() >= 2 || KNOWN_ROOTS.contains(&components[0])
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() < 4 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return false;
    }
    if !matches!(bytes[2], b'\\' | b'/') {
        return false;
    }
    let rest = &path[3..];
    !rest.is_empty()
        && rest
            .split(['\\', '/'])
            .any(|component| !component.is_empty())
}

fn is_windows_unc_path(path: &str) -> bool {
    if !path.starts_with("\\\\") {
        return false;
    }
    let rest = &path[2..];
    let components: Vec<&str> = rest
        .split(['\\', '/'])
        .filter(|component| !component.is_empty())
        .collect();
    if components.is_empty() {
        return false;
    }
    if components[0] == "?" {
        return components.len() >= 2;
    }
    components.len() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_normalization_matches_required_spellings() {
        for key in [
            "token",
            "access_token",
            "apiKey",
            "file_path",
            "filepath",
            "session_id",
            "x-api-key",
            "Authorization",
            "Cookie",
            "client_secret",
            "private_key",
            "password",
            "credential",
            "request-path",
        ] {
            assert!(sensitive_key_kind(key).is_some(), "{key} must be sensitive");
        }
        for key in ["message", "count", "surface", "operation", "revision"] {
            assert!(
                sensitive_key_kind(key).is_none(),
                "{key} must not be sensitive"
            );
        }
    }

    #[test]
    fn values_are_deterministically_redacted() {
        let cases = [
            ("Bearer abc.def.ghi", RedactionKind::Bearer),
            ("Basic YWxpY2U6cGFzcw==", RedactionKind::Basic),
            (
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature",
                RedactionKind::Jwt,
            ),
            (
                "https://alice:password@example.com/callback",
                RedactionKind::UrlUserInfo,
            ),
            (
                "https://example.com/callback?x=1&access_token=canary",
                RedactionKind::UrlQuery,
            ),
            ("/home/alice/private/file.txt", RedactionKind::Path),
            ("/var/tmp/build/output.bin", RedactionKind::Path),
            (r"C:\Users\alice\secret.txt", RedactionKind::Path),
            ("D:/work/private/report.json", RedactionKind::Path),
            (r"\\server\share\private.txt", RedactionKind::Path),
            (r"\\?\C:\Users\alice\secret.txt", RedactionKind::Path),
            ("token=canary-value", RedactionKind::Token),
        ];
        for (input, kind) in cases {
            let redacted = redact_text(input);
            assert!(
                redacted.contains(&kind.marker()),
                "{input} should produce {} but got {redacted}",
                kind.marker()
            );
            assert_eq!(redacted, redact_text(input), "redaction must be stable");
        }
    }

    #[test]
    fn harmless_text_is_preserved() {
        for input in [
            "and/or",
            "24/7",
            "2026/11/11",
            "see docs/SYSTEM_FILES.md",
            "https://example.com/health",
            "name: value",
            "count = 42",
        ] {
            assert_eq!(redact_text(input), input);
        }
    }

    #[test]
    fn sensitive_assignments_and_nested_text_are_covered() {
        let redacted = redact_text("prefix password=hunter2 suffix");
        assert!(redacted.contains("[REDACTED:password]"));
        assert!(!redacted.contains("hunter2"));
    }
}
