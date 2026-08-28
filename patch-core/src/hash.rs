use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

pub(crate) fn validate_sha256(value: &str, label: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{label} must be a lowercase 64-digit SHA-256"
    );
    Ok(())
}

pub(crate) fn require_sha256(bytes: &[u8], expected: &str, label: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    ensure!(
        actual == expected,
        "{label} SHA-256 mismatch: expected {expected}, got {actual}"
    );
    Ok(())
}

#[cfg(test)]
#[path = "hash_tests.rs"]
mod hash_tests;
