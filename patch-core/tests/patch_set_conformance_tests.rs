use pc98_fat12_patcher_core::{
    PatchSetPackageInput, apply_patch_package, create_patch_set, inspect_patch_set,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const ASCII_SOURCE: &[u8] = include_bytes!("../../conformance/source.hdm");
const ASCII_PACKAGE: &[u8] = include_bytes!("../../conformance/package.zip");
const ASCII_TARGET: &[u8] = include_bytes!("../../conformance/target.hdm");
const RAW_SOURCE: &[u8] = include_bytes!("../../conformance/raw-sfn/source.hdm");
const RAW_PACKAGE: &[u8] = include_bytes!("../../conformance/raw-sfn/package.zip");
const RAW_TARGET: &[u8] = include_bytes!("../../conformance/raw-sfn/target.hdm");
const PATCH_SET: &[u8] = include_bytes!("../../conformance/package-set/package.zip");
const MANIFEST: &str = include_str!("../../conformance/package-set/manifest.json");

#[test]
fn native_core_matches_the_public_patch_set_fixture() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(sha256_hex(PATCH_SET), manifest["package_sha256"]);
    assert_eq!(PATCH_SET.len(), manifest["package_bytes"]);

    let inspected = inspect_patch_set(PATCH_SET).unwrap();
    assert_eq!(inspected.manifest.format, manifest["format"]);
    assert_eq!(inspected.manifest.members.len(), 2);
    assert_eq!(
        create_patch_set(
            "public-mixed-package-set-fixture",
            "Public Mixed Package Set Fixture",
            vec![
                PatchSetPackageInput {
                    key: "ascii-disk".to_owned(),
                    label: "ASCII 호환 디스크".to_owned(),
                    package: ASCII_PACKAGE.to_vec(),
                },
                PatchSetPackageInput {
                    key: "raw-sfn-disk".to_owned(),
                    label: "원시 SFN 디스크".to_owned(),
                    package: RAW_PACKAGE.to_vec(),
                },
            ],
        )
        .unwrap(),
        PATCH_SET,
        "patch set authoring must reproduce the fixed ZIP bytes"
    );
    assert_eq!(
        apply_patch_package(ASCII_SOURCE, &inspected.packages["ascii-disk"]).unwrap(),
        ASCII_TARGET
    );
    assert_eq!(
        apply_patch_package(RAW_SOURCE, &inspected.packages["raw-sfn-disk"]).unwrap(),
        RAW_TARGET
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
