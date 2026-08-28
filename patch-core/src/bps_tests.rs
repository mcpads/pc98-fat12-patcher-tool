use super::*;

fn finalize_patch(mut body: Vec<u8>, source: &[u8], target: &[u8]) -> Vec<u8> {
    body.extend_from_slice(&crc32(source).to_le_bytes());
    body.extend_from_slice(&crc32(target).to_le_bytes());
    let checksum = crc32(&body);
    body.extend_from_slice(&checksum.to_le_bytes());
    body
}

fn patch_prefix(source_len: usize, target_len: usize) -> Vec<u8> {
    let mut patch = MAGIC.to_vec();
    encode_number(source_len as u64, &mut patch);
    encode_number(target_len as u64, &mut patch);
    encode_number(0, &mut patch);
    patch
}

#[test]
fn bps_numbers_round_trip_at_boundaries() {
    for value in [
        0,
        1,
        126,
        127,
        128,
        129,
        255,
        256,
        16_383,
        16_384,
        u32::MAX as u64,
    ] {
        let mut encoded = Vec::new();
        encode_number(value, &mut encoded);
        let mut reader = Reader::new(&encoded, 0, encoded.len());
        assert_eq!(reader.read_number().unwrap(), value);
        assert_eq!(reader.position(), encoded.len());
    }
}

#[test]
fn created_patch_reproduces_sparse_and_relocated_data() {
    let source = b"0123456789abcdefghij";
    let target = b"abcdefghij012XX56789";
    let metadata = br#"{"format":"test"}"#;
    let patch = create_patch(source, target, metadata).unwrap();
    let applied = apply_patch(source, &patch).unwrap();
    assert_eq!(applied.target, target);
    assert_eq!(applied.info.metadata, metadata);
    let statistics = inspect_patch_statistics(&patch).unwrap();
    assert_eq!(
        statistics.source_read_bytes
            + statistics.target_read_bytes
            + statistics.source_copy_bytes
            + statistics.target_copy_bytes,
        target.len()
    );
    assert!(statistics.action_count > 1);
}

#[test]
fn decoder_accepts_overlapping_target_copy() {
    let source = b"";
    let target = b"abcabcabc";
    let mut patch = patch_prefix(source.len(), target.len());
    encode_action(TARGET_READ, 3, &mut patch).unwrap();
    patch.extend_from_slice(b"abc");
    encode_action(TARGET_COPY, 6, &mut patch).unwrap();
    encode_number(0, &mut patch);
    let patch = finalize_patch(patch, source, target);
    assert_eq!(apply_patch(source, &patch).unwrap().target, target);
    assert_eq!(
        inspect_patch_statistics(&patch).unwrap(),
        PatchStatistics {
            action_count: 2,
            source_read_bytes: 0,
            target_read_bytes: 3,
            source_copy_bytes: 0,
            target_copy_bytes: 6,
        }
    );
}

#[test]
fn decoder_rejects_wrong_source_and_corrupt_patch() {
    let source = b"source";
    let target = b"target";
    let patch = create_patch(source, target, b"").unwrap();
    let error = apply_patch(b"sourcf", &patch).unwrap_err().to_string();
    assert!(error.contains("source CRC32 mismatch"));

    let mut corrupt = patch;
    corrupt[4] ^= 1;
    let error = inspect_patch(&corrupt).unwrap_err().to_string();
    assert!(error.contains("patch CRC32 mismatch"));
}

#[test]
fn decoder_accepts_negative_source_copy_offsets() {
    let source = b"abcdef";
    let target = b"efab";
    let mut patch = patch_prefix(source.len(), target.len());
    encode_action(SOURCE_COPY, 2, &mut patch).unwrap();
    encode_number(8, &mut patch);
    encode_action(SOURCE_COPY, 2, &mut patch).unwrap();
    encode_number(13, &mut patch);
    let patch = finalize_patch(patch, source, target);
    assert_eq!(apply_patch(source, &patch).unwrap().target, target);
}

#[test]
fn decoder_rejects_metadata_past_the_parse_budget() {
    let source = b"source";
    let target = b"target";
    let metadata = vec![0_u8; MAX_BPS_METADATA_BYTES + 1];
    let patch = create_patch(source, target, &metadata).unwrap();
    let error = inspect_patch(&patch).unwrap_err().to_string();
    assert!(error.contains("BPS metadata is too large"));
}

#[test]
fn statistics_reject_an_action_stream_past_its_work_budget() {
    let source = b"ab";
    let target = b"ab";
    let mut patch = patch_prefix(source.len(), target.len());
    encode_action(SOURCE_READ, 1, &mut patch).unwrap();
    encode_action(SOURCE_READ, 1, &mut patch).unwrap();
    let patch = finalize_patch(patch, source, target);

    let error = inspect_patch_statistics_with_action_limit(&patch, 1)
        .unwrap_err()
        .to_string();
    assert!(error.contains("BPS action count exceeds 1"));
}

#[test]
fn decoder_rejects_copy_ranges_outside_the_available_bytes() {
    let source = b"ab";
    let target = b"x";
    let mut source_copy = patch_prefix(source.len(), target.len());
    encode_action(SOURCE_COPY, 1, &mut source_copy).unwrap();
    encode_number(4, &mut source_copy);
    let source_copy = finalize_patch(source_copy, source, target);
    let error = apply_patch(source, &source_copy).unwrap_err().to_string();
    assert!(error.contains("BPS SourceCopy 2..3 exceeds source"));

    let source = b"";
    let mut target_copy = patch_prefix(source.len(), target.len());
    encode_action(TARGET_COPY, 1, &mut target_copy).unwrap();
    encode_number(0, &mut target_copy);
    let target_copy = finalize_patch(target_copy, source, target);
    let error = apply_patch(source, &target_copy).unwrap_err().to_string();
    assert!(error.contains("BPS TargetCopy offset 0 is not before output position 0"));
}
