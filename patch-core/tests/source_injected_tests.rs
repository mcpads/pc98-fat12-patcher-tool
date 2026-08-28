use std::fs;

use pc98_fat12_patcher_core::{apply_patch_package, create_patch_package, inspect_patch_package};
use sha2::{Digest, Sha256};

fn required_path(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must point to a local test input"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
#[ignore = "requires user-owned source and content HDM files"]
fn exact_source_builds_and_applies_the_declared_file_patch_package() {
    let plan_json =
        fs::read_to_string(required_path("PC98_PATCH_PLAN")).expect("read source-injected plan");
    let source = fs::read(required_path("PC98_PATCH_SOURCE")).expect("read source HDM");
    let content = fs::read(required_path("PC98_PATCH_CONTENT")).expect("read content HDM");

    let package = create_patch_package(&plan_json, &source, &content)
        .expect("create conventional file-patch ZIP");
    let contents = inspect_patch_package(&package).expect("inspect conventional patch ZIP");
    let applied = apply_patch_package(&source, &package).expect("apply conventional patch ZIP");

    assert_eq!(sha256_hex(&applied), contents.recipe.target.sha256);
    eprintln!(
        "source={} bytes, content={} bytes, target={} bytes, file_patches={}, zip={} bytes",
        source.len(),
        content.len(),
        applied.len(),
        contents.patches.len(),
        package.len()
    );
}
