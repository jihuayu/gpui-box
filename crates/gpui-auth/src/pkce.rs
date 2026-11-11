//! Pure-Rust S256 PKCE primitives.
//!
//! The module implements FIPS 180-4 SHA-256 and RFC 4648 §5 unpadded
//! base64url without external crates or unsafe code. A [`PkceVerifier`] is a
//! sensitive value: its bytes are overwritten on drop, its `Debug` output is
//! redacted, and it deliberately implements neither `Clone` nor `Display`.

use std::{fmt, hint::black_box};

/// Minimum verifier length from RFC 7636 §4.1.
pub const MIN_PKCE_VERIFIER_LEN: usize = 43;
/// Maximum verifier length from RFC 7636 §4.1.
pub const MAX_PKCE_VERIFIER_LEN: usize = 128;
/// Length of an S256 challenge in unpadded base64url characters.
pub const PKCE_CHALLENGE_LEN: usize = 43;

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// FIPS 180-4 SHA-256 over `input`.
pub fn sha256(input: &[u8]) -> [u8; 32] {
    let mut state = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let (blocks, remainder) = input.as_chunks::<64>();
    for block in blocks {
        compress(&mut state, block);
    }

    let mut tail = [0u8; 128];
    tail[..remainder.len()].copy_from_slice(remainder);
    tail[remainder.len()] = 0x80;
    let total = if remainder.len() < 56 { 64 } else { 128 };
    let bit_len = (input.len() as u64).wrapping_mul(8);
    tail[total - 8..total].copy_from_slice(&bit_len.to_be_bytes());
    let (tail_blocks, _) = tail[..total].as_chunks::<64>();
    for block in tail_blocks {
        compress(&mut state, block);
    }

    let mut digest = [0u8; 32];
    let (digest_words, _) = digest.as_chunks_mut::<4>();
    for (chunk, word) in digest_words.iter_mut().zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut schedule = [0u32; 64];
    let (words, _) = block.as_chunks::<4>();
    for (index, chunk) in words.iter().enumerate() {
        schedule[index] = u32::from_be_bytes(*chunk);
    }
    for index in 16..64 {
        let s0 = schedule[index - 15].rotate_right(7)
            ^ schedule[index - 15].rotate_right(18)
            ^ (schedule[index - 15] >> 3);
        let s1 = schedule[index - 2].rotate_right(17)
            ^ schedule[index - 2].rotate_right(19)
            ^ (schedule[index - 2] >> 10);
        schedule[index] = schedule[index - 16]
            .wrapping_add(s0)
            .wrapping_add(schedule[index - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..64 {
        let big_sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(big_sigma1)
            .wrapping_add(choose)
            .wrapping_add(SHA256_K[index])
            .wrapping_add(schedule[index]);
        let big_sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = big_sigma0.wrapping_add(majority);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// RFC 4648 §5 unpadded base64url encoding.
pub fn base64url_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = chunk.get(1).copied().map(u32::from).unwrap_or(0);
        let third = chunk.get(2).copied().map(u32::from).unwrap_or(0);
        let combined = (first << 16) | (second << 8) | third;
        output.push(BASE64URL[((combined >> 18) & 0x3f) as usize] as char);
        output.push(BASE64URL[((combined >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            output.push(BASE64URL[((combined >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            output.push(BASE64URL[(combined & 0x3f) as usize] as char);
        }
    }
    output
}

/// A validated, sensitive PKCE code verifier.
///
/// Accepted characters and length follow RFC 7636 §4.1:
/// `[A-Z] / [a-z] / [0-9] / "-" / "." / "_" / "~"`, 43..=128 bytes.
/// The internal buffer is overwritten on drop and `Debug` prints
/// `PkceVerifier([REDACTED])`.
pub struct PkceVerifier {
    bytes: Vec<u8>,
}

impl PkceVerifier {
    /// Validate a caller-supplied verifier.
    pub fn new(value: impl AsRef<str>) -> Result<Self, PkceError> {
        let value = value.as_ref();
        if !(MIN_PKCE_VERIFIER_LEN..=MAX_PKCE_VERIFIER_LEN).contains(&value.len()) {
            return Err(PkceError::InvalidVerifierLength);
        }
        if !value.bytes().all(is_unreserved) {
            return Err(PkceError::InvalidVerifierCharacters);
        }
        Ok(Self {
            bytes: value.as_bytes().to_vec(),
        })
    }

    /// Explicit sensitive access for the code exchange or cold-store write.
    /// Callers must not format, log or retain the returned slice.
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.bytes).unwrap_or_default()
    }

    /// Compute the S256 challenge for this verifier.
    pub fn challenge(&self) -> PkceChallenge {
        PkceChallenge(base64url_encode(&sha256(&self.bytes)))
    }

    /// Consume the verifier and produce a challenge/verifier pair.
    pub fn into_pair(self) -> PkcePair {
        let challenge = self.challenge();
        PkcePair {
            verifier: self,
            challenge,
        }
    }
}

impl fmt::Debug for PkceVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PkceVerifier([REDACTED])")
    }
}

impl PartialEq for PkceVerifier {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.bytes, &other.bytes)
    }
}

impl Eq for PkceVerifier {}

impl Drop for PkceVerifier {
    fn drop(&mut self) {
        self.bytes.fill(0);
        black_box(&self.bytes);
    }
}

/// A validated S256 code challenge in unpadded base64url form.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PkceChallenge(String);

impl PkceChallenge {
    /// Validate an existing S256 challenge.
    pub fn new(value: impl AsRef<str>) -> Result<Self, PkceError> {
        let value = value.as_ref();
        if value.len() != PKCE_CHALLENGE_LEN
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(PkceError::InvalidChallenge);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PkceChallenge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PkceChallenge")
            .field(&self.0)
            .finish()
    }
}

/// A verifier and its derived S256 challenge.
pub struct PkcePair {
    verifier: PkceVerifier,
    challenge: PkceChallenge,
}

impl PkcePair {
    /// Derive the challenge from a validated verifier.
    pub fn from_verifier(verifier: PkceVerifier) -> Self {
        verifier.into_pair()
    }

    pub fn verifier(&self) -> &PkceVerifier {
        &self.verifier
    }

    pub fn challenge(&self) -> &PkceChallenge {
        &self.challenge
    }

    pub fn into_parts(self) -> (PkceVerifier, PkceChallenge) {
        (self.verifier, self.challenge)
    }
}

impl fmt::Debug for PkcePair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PkcePair")
            .field("verifier", &"[REDACTED]")
            .field("challenge", &self.challenge)
            .finish()
    }
}

/// PKCE validation failures. Messages are static and value-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkceError {
    InvalidVerifierLength,
    InvalidVerifierCharacters,
    InvalidChallenge,
}

impl PkceError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidVerifierLength => "PKCE verifier length is outside 43..=128",
            Self::InvalidVerifierCharacters => "PKCE verifier contains a non-unreserved byte",
            Self::InvalidChallenge => "PKCE S256 challenge is malformed",
        }
    }
}

impl fmt::Display for PkceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for PkceError {}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// Length-independent comparison for equal-length public/sensitive byte slices.
pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        let mut out = String::new();
        for byte in bytes {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }

    #[test]
    fn sha256_matches_fips_vectors() {
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
    }

    #[test]
    fn base64url_matches_rfc_vectors() {
        assert_eq!(base64url_encode(b""), "");
        assert_eq!(base64url_encode(b"f"), "Zg");
        assert_eq!(base64url_encode(b"fo"), "Zm8");
        assert_eq!(base64url_encode(b"foo"), "Zm9v");
        assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn rfc_7636_appendix_b_vector() {
        let verifier = PkceVerifier::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
            .expect("valid verifier");
        assert_eq!(
            verifier.challenge().as_str(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(format!("{verifier:?}"), "PkceVerifier([REDACTED])");
    }

    #[test]
    fn rejects_out_of_range_and_unsafe_verifiers() {
        assert_eq!(
            PkceVerifier::new("a".repeat(42)).unwrap_err(),
            PkceError::InvalidVerifierLength
        );
        assert_eq!(
            PkceVerifier::new("a".repeat(129)).unwrap_err(),
            PkceError::InvalidVerifierLength
        );
        assert_eq!(
            PkceVerifier::new("a".repeat(42) + "!").unwrap_err(),
            PkceError::InvalidVerifierCharacters
        );
    }

    #[test]
    fn challenge_is_strict() {
        assert!(PkceChallenge::new("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM").is_ok());
        assert!(PkceChallenge::new("short").is_err());
        assert!(PkceChallenge::new("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-c=").is_err());
    }
}
