use pc98_fat12_patcher_core::{apply_patch_package, create_patch_package, inspect_patch_package};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] = include_bytes!("../../conformance/source.hdm");
const PLAN: &str = include_str!("../../conformance/plan.json");
const PACKAGE: &[u8] = include_bytes!("../../conformance/package.zip");
const TARGET: &[u8] = include_bytes!("../../conformance/target.hdm");
const MANIFEST: &str = include_str!("../../conformance/manifest.json");

#[test]
fn native_core_matches_the_public_conformance_fixture() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(sha256_hex(SOURCE), manifest["source_sha256"]);
    assert_eq!(sha256_hex(PACKAGE), manifest["package_sha256"]);
    assert_eq!(sha256_hex(TARGET), manifest["target_sha256"]);

    let inspected = inspect_patch_package(PACKAGE).unwrap();
    assert_eq!(inspected.recipe.source.sha256, manifest["source_sha256"]);
    assert_eq!(inspected.recipe.target.sha256, manifest["target_sha256"]);
    assert_eq!(
        create_patch_package(PLAN, SOURCE, TARGET).unwrap(),
        PACKAGE,
        "package authoring must reproduce the fixed ZIP bytes"
    );
    assert_eq!(apply_patch_package(SOURCE, PACKAGE).unwrap(), TARGET);
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
