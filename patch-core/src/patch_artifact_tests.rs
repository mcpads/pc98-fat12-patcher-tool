use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use super::*;
use crate::patch_set::{PatchSetPackageInput, create_patch_set};
use crate::test_support::patch_fixture;

#[test]
fn single_artifact_classifies_only_exact_source_and_target_hashes() {
    let (source, package, target) = patch_fixture(1);
    let definition = patch_artifact_definition(&package).unwrap();
    assert_eq!(definition.kind, PatchArtifactKind::Single);
    assert_eq!(definition.members[0].key, SINGLE_ARTIFACT_MEMBER_KEY);
    assert_eq!(
        classify_patch_artifact_input(&source, &package).unwrap(),
        PatchArtifactInputMatch::Source {
            member_key: SINGLE_ARTIFACT_MEMBER_KEY.to_owned()
        }
    );
    assert_eq!(
        classify_patch_artifact_input(&target, &package).unwrap(),
        PatchArtifactInputMatch::Target {
            member_key: SINGLE_ARTIFACT_MEMBER_KEY.to_owned()
        }
    );

    let mut same_size_wrong_hash = source.clone();
    let last = same_size_wrong_hash.len() - 1;
    same_size_wrong_hash[last] ^= 1;
    assert_eq!(
        classify_patch_artifact_input(&same_size_wrong_hash, &package).unwrap(),
        PatchArtifactInputMatch::Unsupported
    );
}

#[test]
fn set_artifact_matches_reordered_inputs_by_hash_and_materializes_every_member() {
    let (source_a, package_a, target_a) = patch_fixture(1);
    let (source_b, package_b, target_b) = patch_fixture(2);
    let patch_set = create_patch_set(
        "fixture-set",
        "Fixture Set",
        vec![
            PatchSetPackageInput {
                key: "disk-a".to_owned(),
                label: "첫 장".to_owned(),
                package: package_a,
            },
            PatchSetPackageInput {
                key: "disk-b".to_owned(),
                label: "둘째 장".to_owned(),
                package: package_b,
            },
        ],
    )
    .unwrap();
    let definition = patch_artifact_definition(&patch_set).unwrap();
    assert_eq!(definition.kind, PatchArtifactKind::Set);
    assert_eq!(
        classify_patch_artifact_input(&source_b, &patch_set).unwrap(),
        PatchArtifactInputMatch::Source {
            member_key: "disk-b".to_owned()
        }
    );
    assert_eq!(
        classify_patch_artifact_input(&target_a, &patch_set).unwrap(),
        PatchArtifactInputMatch::Target {
            member_key: "disk-a".to_owned()
        }
    );
    assert_eq!(
        materialize_patch_artifact_member(&source_a, &patch_set, "disk-a").unwrap(),
        target_a
    );
    assert_eq!(
        materialize_patch_artifact_member(&source_b, &patch_set, "disk-b").unwrap(),
        target_b
    );
    assert_eq!(
        materialize_patch_artifact_member(&target_b, &patch_set, "disk-b").unwrap(),
        target_b,
        "an exact target is already complete and must pass through unchanged"
    );
}

#[test]
fn artifact_rejects_archives_with_both_root_markers() {
    let (_, package, _) = patch_fixture(1);
    let definition = patch_artifact_definition(&package).unwrap();
    let manifest = serde_json::json!({
        "format": crate::PATCH_SET_FORMAT,
        "id": "ambiguous",
        "title": "Ambiguous",
        "members": [{
            "key": "member",
            "label": "Member",
            "package_size": package.len(),
            "package_sha256": definition.members[0].source_sha256
        }]
    });
    let bytes = write_root_entries(&[
        (crate::RECIPE_ENTRY_NAME, b"{}"),
        (crate::PATCH_SET_ENTRY_NAME, manifest.to_string().as_bytes()),
    ]);

    let error = inspect_patch_artifact(&bytes).unwrap_err().to_string();
    assert!(error.contains("exactly one root marker"));
}

fn write_root_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let output = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default());
    for (name, contents) in entries {
        archive.start_file(*name, options).unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
