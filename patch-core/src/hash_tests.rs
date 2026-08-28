use super::*;

#[test]
fn sha256_matches_the_published_empty_input_vector() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_field_rejects_uppercase_and_wrong_length() {
    let uppercase = "A".repeat(64);
    assert!(validate_sha256(&uppercase, "source").is_err());
    assert!(validate_sha256("abc", "source").is_err());
}
