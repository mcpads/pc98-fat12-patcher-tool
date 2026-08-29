use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

use super::*;
use crate::apply_patch_package;
use crate::test_support::patch_fixture;

fn fixture_members() -> (Vec<PatchSetPackageInput>, Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let (source_a, package_a, target_a) = patch_fixture(1);
    let (source_b, package_b, target_b) = patch_fixture(2);
    (
        vec![
            PatchSetPackageInput {
                key: "disk-a".to_owned(),
                label: "디스크 A".to_owned(),
                package: package_a,
            },
            PatchSetPackageInput {
                key: "disk-b".to_owned(),
                label: "디스크 B".to_owned(),
                package: package_b,
            },
        ],
        vec![source_a, source_b],
        vec![target_a, target_b],
    )
}

#[test]
fn patch_set_reproducibly_wraps_exact_existing_packages() {
    let (members, sources, targets) = fixture_members();
    let packages = members
        .iter()
        .map(|member| (member.key.clone(), member.package.clone()))
        .collect::<BTreeMap<_, _>>();
    let patch_set = create_patch_set("fixture-set", "Fixture Set", members.clone()).unwrap();
    let second = create_patch_set("fixture-set", "Fixture Set", members).unwrap();

    assert_eq!(patch_set, second);
    let inspected = inspect_patch_set(&patch_set).unwrap();
    assert_eq!(inspected.packages, packages);
    assert_eq!(inspected.manifest.members.len(), 2);
    for (index, key) in ["disk-a", "disk-b"].iter().enumerate() {
        assert_eq!(
            apply_patch_package(&sources[index], &inspected.packages[*key]).unwrap(),
            targets[index]
        );
    }
}

#[test]
fn patch_set_rejects_ambiguous_image_hashes_even_when_keys_and_labels_differ() {
    let (_, package, _) = patch_fixture(1);
    let error = create_patch_set(
        "ambiguous-set",
        "Ambiguous Set",
        vec![
            PatchSetPackageInput {
                key: "first".to_owned(),
                label: "첫 장".to_owned(),
                package: package.clone(),
            },
            PatchSetPackageInput {
                key: "second".to_owned(),
                label: "둘째 장".to_owned(),
                package,
            },
        ],
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("image SHA-256 is ambiguous"));
}

#[test]
fn patch_set_rejects_changed_or_undeclared_nested_packages() {
    let (members, _, _) = fixture_members();
    let valid = create_patch_set("fixture-set", "Fixture Set", members).unwrap();
    let inspected = inspect_patch_set(&valid).unwrap();
    let mut changed = inspected.packages["disk-a"].clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    let invalid = write_set_entries(&[
        (PATCH_SET_ENTRY_NAME, inspected.manifest_json.as_bytes()),
        (&package_entry_name("disk-a"), &changed),
        (&package_entry_name("disk-b"), &inspected.packages["disk-b"]),
    ]);
    let error = inspect_patch_set(&invalid).unwrap_err().to_string();
    assert!(error.contains("package SHA-256 mismatch"));

    let extra = write_set_entries(&[
        (PATCH_SET_ENTRY_NAME, inspected.manifest_json.as_bytes()),
        (&package_entry_name("disk-a"), &inspected.packages["disk-a"]),
        (&package_entry_name("disk-b"), &inspected.packages["disk-b"]),
        ("packages/extra.zip", &inspected.packages["disk-a"]),
    ]);
    assert!(
        inspect_patch_set(&extra)
            .unwrap_err()
            .to_string()
            .contains("package entries differ")
    );
}

#[test]
fn patch_set_manifest_rejects_unknown_fields_and_unsafe_keys() {
    let (members, _, _) = fixture_members();
    let patch_set = create_patch_set("fixture-set", "Fixture Set", members).unwrap();
    let inspected = inspect_patch_set(&patch_set).unwrap();
    let mut value = serde_json::to_value(inspected.manifest).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("revision".to_owned(), 1.into());
    assert!(parse_patch_set_manifest(&value.to_string()).is_err());

    let mut manifest: PatchSetManifest = serde_json::from_value({
        let mut value: serde_json::Value = serde_json::from_str(&inspected.manifest_json).unwrap();
        value["members"][0]["key"] = "../disk".into();
        value
    })
    .unwrap();
    manifest.format = PATCH_SET_FORMAT.to_owned();
    assert!(
        manifest
            .validate()
            .unwrap_err()
            .to_string()
            .contains("safe ASCII")
    );
}

fn write_set_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
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
