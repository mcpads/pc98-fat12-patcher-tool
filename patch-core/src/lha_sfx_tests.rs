use super::*;

#[test]
fn mz_length_uses_a_partial_final_page() {
    let mut executable = vec![0_u8; 2_000];
    executable[..2].copy_from_slice(b"MZ");
    executable[2..4].copy_from_slice(&409_u16.to_le_bytes());
    executable[4..6].copy_from_slice(&4_u16.to_le_bytes());
    assert_eq!(mz_executable_length(&executable).unwrap(), 1_945);
}

#[test]
fn mz_length_uses_a_full_final_page_when_remainder_is_zero() {
    let mut executable = vec![0_u8; 2_048];
    executable[..2].copy_from_slice(b"MZ");
    executable[4..6].copy_from_slice(&3_u16.to_le_bytes());
    assert_eq!(mz_executable_length(&executable).unwrap(), 1_536);
}

#[test]
fn mz_length_rejects_a_header_without_an_appended_archive() {
    let mut executable = vec![0_u8; 512];
    executable[..2].copy_from_slice(b"MZ");
    executable[4..6].copy_from_slice(&1_u16.to_le_bytes());
    let error = mz_executable_length(&executable).unwrap_err().to_string();
    assert!(error.contains("does not leave an appended archive"));
}
