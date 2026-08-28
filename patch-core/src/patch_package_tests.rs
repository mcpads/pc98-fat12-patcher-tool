use std::io::{Cursor, Write};

use zip::ZipArchive;

use super::*;
use crate::pipeline::create_package_contents;
use crate::recipe::PACKAGE_FORMAT;
use crate::test_support::{content_image, direct_root_plan, fixture_image};

fn fixture_product(include_directory: bool) -> (Vec<u8>, crate::recipe::PatchPlan, Vec<u8>) {
    let retained = b"system";
    let payload = b"original game payload";
    let localized = b"localized game payload";
    let source = fixture_image(
        &[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)],
        include_directory,
    );
    let plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let content = content_image(&source, &plan, &[("GAME.COM", localized)]);
    (source, plan, content)
}

#[test]
fn package_contains_recipe_and_one_patch_per_changed_logical_file() {
    let (source, plan, content) = fixture_product(false);
    let plan_json = serde_json::to_string_pretty(&plan).unwrap();
    let package = create_patch_package(&plan_json, &source, &content).unwrap();
    let second_package = create_patch_package(&plan_json, &source, &content).unwrap();

    assert_eq!(
        package, second_package,
        "package creation must be reproducible"
    );
    let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
    let mut names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [patch_entry_name("GAME.COM"), RECIPE_ENTRY_NAME.to_owned()]
    );
}

#[test]
fn raw_sfn_package_uses_a_safe_patch_key_and_writes_exact_name_bytes() {
    let retained = b"system";
    let payload = b"original game payload";
    let localized = b"localized docho payload";
    let source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], false);
    let mut plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.DAT",
        payload,
    );
    let raw_name = crate::FatShortName::Raw {
        raw_sfn_hex: "93b9919088d995b7444154".to_owned(),
    };
    plan.format = Some(PACKAGE_FORMAT.to_owned());
    plan.assembly.placed_files[0].patch_key = Some("DOCHO-DATA".to_owned());
    plan.assembly.placed_files[0].name = raw_name.clone();
    let content = content_image(&source, &plan, &[("DOCHO-DATA", localized)]);
    let plan_json = serde_json::to_string_pretty(&plan).unwrap();

    let package = create_patch_package(&plan_json, &source, &content).unwrap();
    let contents = inspect_patch_package(&package).unwrap();
    let applied = apply_patch_package(&source, &package).unwrap();

    assert_eq!(contents.recipe.format, PACKAGE_FORMAT);
    assert_eq!(contents.recipe.assembly.placed_files[0].name, raw_name);
    assert!(contents.patches.contains_key("DOCHO-DATA"));
    let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
    let mut names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [patch_entry_name("DOCHO-DATA"), RECIPE_ENTRY_NAME.to_owned()]
    );
    let requested = BTreeSet::from([contents.recipe.assembly.placed_files[0]
        .name
        .raw_bytes("test output name")
        .unwrap()]);
    let files =
        crate::fat12::read_root_files(&applied, contents.recipe.source.mount_policy, &requested)
            .unwrap();
    assert_eq!(files.values().next().unwrap(), localized);
}

#[test]
fn package_creation_ignores_content_image_bytes_outside_logical_files() {
    let (source, plan, content) = fixture_product(false);
    let mut altered_slack = content.clone();
    let last = altered_slack.len() - 1;
    altered_slack[last] ^= 0x5a;
    let plan_json = serde_json::to_string_pretty(&plan).unwrap();

    let first = create_patch_package(&plan_json, &source, &content).unwrap();
    let second = create_patch_package(&plan_json, &source, &altered_slack).unwrap();

    assert_eq!(
        first, second,
        "content-image slack and unallocated bytes are not package inputs"
    );
}

#[test]
fn package_applies_to_exact_source_and_removes_unlisted_directories() {
    let (source, plan, content) = fixture_product(true);
    let plan_json = serde_json::to_string_pretty(&plan).unwrap();
    let package = create_patch_package(&plan_json, &source, &content).unwrap();
    let contents = inspect_patch_package(&package).unwrap();
    let applied = apply_patch_package(&source, &package).unwrap();

    assert_eq!(contents.patches.len(), 1);
    assert_eq!(
        crate::hash::sha256_hex(&applied),
        contents.recipe.target.sha256
    );
}

#[test]
fn package_ignores_an_unrelated_document_without_extracting_it() {
    let (source, plan, content) = fixture_product(false);
    let created = create_package_contents(plan, &source, &content).unwrap();
    let patch = created.patches.get("GAME.COM").unwrap();
    let package = write_package_entries(&[
        (RECIPE_ENTRY_NAME, created.recipe_json.as_bytes()),
        (&patch_entry_name("GAME.COM"), patch),
        ("README.txt", b"human-readable release notes"),
    ]);

    assert!(apply_patch_package(&source, &package).is_ok());
}

#[test]
fn package_requires_the_conventional_recipe_name_at_the_archive_root() {
    let package = write_package_entries(&[("patch/recipe.json", b"{}")]);
    let error = inspect_patch_package(&package).unwrap_err().to_string();
    assert!(error.contains("missing root entry recipe.json"));
}

#[test]
fn web_reader_hides_parser_internals_for_an_old_whole_image_package() {
    let old_recipe = br#"{
        "id": "old-whole-image-package",
        "title": "Old whole-image package",
        "output_filename": "patched.hdm",
        "assembly": {
            "baseline_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "retained_files": [],
            "placed_files": []
        }
    }"#;
    let package = write_package_entries(&[(RECIPE_ENTRY_NAME, old_recipe)]);

    let error = crate::read_patch_package_recipe_for_web(&package).unwrap_err();

    assert_eq!(
        error,
        "이 패치 ZIP은 현재 지원하는 파일별 패치 형식이 아닙니다. 이 패처용 ZIP을 선택하세요."
    );
    assert!(!error.contains("baseline_sha256"));
    assert!(!error.contains("unknown field"));
    assert!(!error.contains("line"));
}

#[test]
fn web_reader_hides_archive_internals_for_a_non_zip_file() {
    let error = crate::read_patch_package_recipe_for_web(b"not a ZIP file").unwrap_err();

    assert_eq!(
        error,
        "패치 ZIP을 읽을 수 없습니다. 올바른 PC-98 FAT12 패치 ZIP인지 확인하세요."
    );
    assert!(!error.contains("open patch ZIP"));
    assert!(!error.contains("invalid Zip archive"));
}

#[test]
fn package_requires_exactly_the_declared_file_patch_set() {
    let (source, plan, content) = fixture_product(false);
    let created = create_package_contents(plan, &source, &content).unwrap();
    let missing = write_package_entries(&[(RECIPE_ENTRY_NAME, created.recipe_json.as_bytes())]);
    assert!(
        inspect_patch_package(&missing)
            .unwrap_err()
            .to_string()
            .contains("file-patch entries differ")
    );

    let patch = created.patches.get("GAME.COM").unwrap();
    let extra = write_package_entries(&[
        (RECIPE_ENTRY_NAME, created.recipe_json.as_bytes()),
        (&patch_entry_name("GAME.COM"), patch),
        ("patches/OTHER.COM.bps", patch),
    ]);
    assert!(
        inspect_patch_package(&extra)
            .unwrap_err()
            .to_string()
            .contains("file-patch entries differ")
    );
}

#[test]
fn package_rejects_a_bps_bound_to_another_recipe() {
    let (source, plan, content) = fixture_product(false);
    let created = create_package_contents(plan, &source, &content).unwrap();
    let mut other_recipe = created.recipe;
    other_recipe.id = "another-patch".to_owned();
    let other_json = format!("{}\n", serde_json::to_string_pretty(&other_recipe).unwrap());
    let package = write_patch_package(other_json.as_bytes(), &created.patches).unwrap();

    let error = format!("{:#}", inspect_patch_package(&package).unwrap_err());
    assert!(error.contains("metadata"));
}

#[test]
fn package_rejects_a_source_that_does_not_match_the_recipe() {
    let (mut source, plan, content) = fixture_product(false);
    let plan_json = serde_json::to_string_pretty(&plan).unwrap();
    let package = create_patch_package(&plan_json, &source, &content).unwrap();
    let last = source.len() - 1;
    source[last] ^= 1;

    let error = apply_patch_package(&source, &package)
        .unwrap_err()
        .to_string();
    assert!(error.contains("source image SHA-256 mismatch"));
}

#[test]
fn package_rejects_a_recipe_entry_that_exceeds_its_decode_budget() {
    let oversized_recipe = vec![b' '; MAX_RECIPE_BYTES + 1];
    let package = write_package_entries(&[(RECIPE_ENTRY_NAME, &oversized_recipe)]);

    let error = inspect_patch_package(&package).unwrap_err().to_string();
    assert!(error.contains("recipe.json is too large"));
}

fn write_package_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
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
