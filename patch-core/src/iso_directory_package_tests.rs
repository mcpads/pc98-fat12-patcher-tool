use super::*;
use crate::iso9660::{LOGICAL_SECTOR_SIZE, extract_logical_directory};

fn fixture() -> (String, Vec<u8>, BTreeMap<String, Vec<u8>>) {
    let source_files = vec![
        IsoFile {
            name: "456.BAT".to_owned(),
            bytes: b"MADO456\r\n".to_vec(),
        },
        IsoFile {
            name: "MADO456.COM".to_owned(),
            bytes: vec![0x45; 4_111],
        },
    ];
    let logical = build_single_directory_iso("DS_VOL_9", "MADOU456", &source_files).unwrap();
    let source = raw_mode1_image(&logical);
    let mut content = source_files
        .iter()
        .map(|file| (file.name.clone(), file.bytes.clone()))
        .collect::<BTreeMap<_, _>>();
    content
        .get_mut("MADO456.COM")
        .unwrap()
        .splice(0..4, [0x4b, 0x4f, 0x52, 0x21]);
    let plan = IsoDirectoryPatchPlan {
        format: ISO_DIRECTORY_PACKAGE_FORMAT.to_owned(),
        id: "fixture-iso-directory".to_owned(),
        title: "Fixture ISO Directory".to_owned(),
        output_filename: "fixture-ko.iso".to_owned(),
        source: IsoDirectorySource {
            size: source.len(),
            sha256: sha256_hex(&source),
            volume_id: "DS_VOL_9".to_owned(),
            directory: "MADOU456".to_owned(),
            expected_file_count: source_files.len(),
            expected_total_size: source_files.iter().map(|file| file.bytes.len()).sum(),
        },
        target: PlannedIsoDirectoryTarget {
            volume_id: "MADOU456_KO".to_owned(),
            directory: "MADOU456".to_owned(),
        },
    };
    (serde_json::to_string(&plan).unwrap(), source, content)
}

#[test]
fn creates_and_applies_a_deterministic_directory_patch() {
    let (plan, source, content) = fixture();
    let first = create_iso_directory_patch_package(&plan, &source, &content).unwrap();
    let second = create_iso_directory_patch_package(&plan, &source, &content).unwrap();
    assert_eq!(first, second);

    let inspected = inspect_iso_directory_patch_package(&first).unwrap();
    assert_eq!(inspected.recipe.files.len(), 2);
    assert_eq!(
        inspected.patches.keys().collect::<Vec<_>>(),
        ["MADO456.COM"]
    );
    let target = apply_iso_directory_patch_package(&source, &first).unwrap();
    assert_eq!(target.len(), inspected.recipe.target.size);
    assert_eq!(sha256_hex(&target), inspected.recipe.target.sha256);
    assert!(matches!(
        crate::inspect_patch_artifact(&first).unwrap(),
        crate::PatchArtifact::IsoDirectory(_)
    ));
    assert_eq!(
        crate::classify_patch_artifact_input(&source, &first).unwrap(),
        crate::PatchArtifactInputMatch::Source {
            member_key: crate::SINGLE_ARTIFACT_MEMBER_KEY.to_owned()
        }
    );
    assert_eq!(
        crate::materialize_patch_artifact_member(
            &source,
            &first,
            crate::SINGLE_ARTIFACT_MEMBER_KEY,
        )
        .unwrap(),
        target
    );
    let extracted = extract_logical_directory(&target, "MADOU456_KO", "MADOU456").unwrap();
    assert_eq!(
        extracted
            .into_iter()
            .map(|file| (file.name, file.bytes))
            .collect::<BTreeMap<_, _>>(),
        content
    );
}

#[test]
fn refuses_an_exact_source_identity_mismatch() {
    let (plan, mut source, content) = fixture();
    source[0] ^= 1;
    let error = create_iso_directory_patch_package(&plan, &source, &content)
        .unwrap_err()
        .to_string();
    assert!(error.contains("source CD image SHA-256 mismatch"));
}

#[test]
fn refuses_extra_content_files() {
    let (plan, source, mut content) = fixture();
    content.insert("EXTRA.DAT".to_owned(), vec![1]);
    let error = create_iso_directory_patch_package(&plan, &source, &content)
        .unwrap_err()
        .to_string();
    assert!(error.contains("content directory filenames differ"));
}

fn raw_mode1_image(logical: &[u8]) -> Vec<u8> {
    let mut raw = vec![0u8; logical.len() / LOGICAL_SECTOR_SIZE * RAW_MODE1_SECTOR_SIZE];
    let (logical_sectors, remainder) = logical.as_chunks::<LOGICAL_SECTOR_SIZE>();
    assert!(remainder.is_empty());
    for (lba, logical_sector) in logical_sectors.iter().enumerate() {
        let raw_sector = &mut raw[lba * RAW_MODE1_SECTOR_SIZE..(lba + 1) * RAW_MODE1_SECTOR_SIZE];
        raw_sector[0] = 0;
        raw_sector[1..11].fill(0xff);
        raw_sector[11] = 0;
        raw_sector[15] = 1;
        raw_sector[16..16 + LOGICAL_SECTOR_SIZE].copy_from_slice(logical_sector);
    }
    raw
}
