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

#[test]
fn extractor_matches_a_non_ascii_member_by_its_raw_lha_name() {
    let raw_name = [
        0x93, 0xb9, 0x91, 0x90, 0x88, 0xd9, 0x95, 0xb7, b'.', b'D', b'A', b'T',
    ];
    let payload = b"docho fixture payload";
    let executable = mz_with_stored_lha_member(&raw_name, payload);
    let selector = LhaMemberName::Raw {
        raw_name_hex: "93b9919088d995b72e444154".to_owned(),
    };
    let expected = BTreeMap::from([(selector.clone(), payload.len())]);

    let extracted = extract_mz_lha_members(&executable, &expected).unwrap();

    assert_eq!(extracted.get(&selector).unwrap(), payload);
}

fn mz_with_stored_lha_member(raw_name: &[u8], payload: &[u8]) -> Vec<u8> {
    let executable_size = 512_usize;
    let mut executable = vec![0_u8; executable_size];
    executable[..2].copy_from_slice(b"MZ");
    executable[4..6].copy_from_slice(&1_u16.to_le_bytes());

    let payload_size = u32::try_from(payload.len()).unwrap();
    let mut header_body = Vec::new();
    header_body.extend_from_slice(b"-lh0-");
    header_body.extend_from_slice(&payload_size.to_le_bytes());
    header_body.extend_from_slice(&payload_size.to_le_bytes());
    header_body.extend_from_slice(&0_u32.to_le_bytes());
    header_body.push(0x20);
    header_body.push(0);
    header_body.push(u8::try_from(raw_name.len()).unwrap());
    header_body.extend_from_slice(raw_name);
    header_body.extend_from_slice(&lha_crc16(payload).to_le_bytes());

    executable.push(u8::try_from(header_body.len()).unwrap());
    executable.push(header_body.iter().copied().fold(0_u8, u8::wrapping_add));
    executable.extend_from_slice(&header_body);
    executable.extend_from_slice(payload);
    executable.push(0);
    executable
}

fn lha_crc16(bytes: &[u8]) -> u16 {
    bytes.iter().copied().fold(0_u16, |mut crc, byte| {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xa001
            };
        }
        crc
    })
}
