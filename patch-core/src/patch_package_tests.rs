use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use fatfs::{FileSystem, FsOptions};
use zip::ZipArchive;

use super::*;
use crate::fat12::assemble_baseline;
use crate::hash::sha256_hex;
use crate::test_support::{direct_root_recipe, fixture_image};

fn fixture_product() -> (Vec<u8>, PatchRecipe, Vec<u8>) {
    let retained = b"system";
    let payload = b"original game payload";
    let source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], false);
    let mut recipe = direct_root_recipe(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let placed = BTreeMap::from([("GAME.COM".to_owned(), payload.to_vec())]);
    let baseline = assemble_baseline(&source, &recipe, &placed).unwrap();
    recipe.assembly.baseline_sha256 = sha256_hex(&baseline);

    let mut target = baseline.clone();
    {
        let filesystem =
            FileSystem::new(Cursor::new(target.as_mut_slice()), FsOptions::new()).unwrap();
        let root = filesystem.root_dir();
        let mut game = root.open_file("GAME.COM").unwrap();
        game.write_all(b"KOREAN!!").unwrap();
        drop(game);
        drop(root);
        filesystem.unmount().unwrap();
    }
    recipe.target.sha256 = sha256_hex(&target);
    (source, recipe, target)
}

#[test]
fn package_contains_only_the_two_conventional_root_entries() {
    let (source, recipe, target) = fixture_product();
    let recipe_json = serde_json::to_string_pretty(&recipe).unwrap();
    let package = create_patch_package(&recipe_json, &source, &target).unwrap();
    let second_package = create_patch_package(&recipe_json, &source, &target).unwrap();

    assert_eq!(
        package, second_package,
        "package creation must be reproducible"
    );
    let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
    let mut names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, [BPS_ENTRY_NAME, RECIPE_ENTRY_NAME]);
}

#[test]
fn package_rebuilds_the_supported_source_and_applies_its_bps() {
    let (source, recipe, target) = fixture_product();
    let recipe_json = serde_json::to_string_pretty(&recipe).unwrap();
    let package = create_patch_package(&recipe_json, &source, &target).unwrap();

    let contents = inspect_patch_package(&package).unwrap();
    assert_eq!(contents.recipe, recipe);
    assert_eq!(contents.recipe_json, recipe_json);
    assert_eq!(apply_patch_package(&source, &package).unwrap(), target);
}

#[test]
fn package_ignores_an_unrelated_document_without_extracting_it() {
    let (source, recipe, target) = fixture_product();
    let recipe_json = serde_json::to_string_pretty(&recipe).unwrap();
    let patch = create_recipe_patch(&recipe, &source, &target).unwrap();
    let package = write_package_entries(&[
        (RECIPE_ENTRY_NAME, recipe_json.as_bytes()),
        (BPS_ENTRY_NAME, &patch),
        ("README.txt", b"human-readable release notes"),
    ]);

    assert_eq!(apply_patch_package(&source, &package).unwrap(), target);
}

#[test]
fn package_requires_the_conventional_recipe_name_at_the_archive_root() {
    let (source, recipe, target) = fixture_product();
    let recipe_json = serde_json::to_string_pretty(&recipe).unwrap();
    let patch = create_recipe_patch(&recipe, &source, &target).unwrap();
    let package = write_package_entries(&[
        ("patch/recipe.json", recipe_json.as_bytes()),
        (BPS_ENTRY_NAME, &patch),
    ]);

    let error = inspect_patch_package(&package).unwrap_err().to_string();
    assert!(error.contains("missing root entry recipe.json"));
}

#[test]
fn package_rejects_a_bps_bound_to_another_recipe() {
    let (source, recipe, target) = fixture_product();
    let recipe_json = serde_json::to_string_pretty(&recipe).unwrap();
    let mut other_recipe = recipe.clone();
    other_recipe.id = "another-patch".to_owned();
    let other_patch = create_recipe_patch(&other_recipe, &source, &target).unwrap();
    let package = write_patch_package(recipe_json.as_bytes(), &other_patch).unwrap();

    let error = format!("{:#}", inspect_patch_package(&package).unwrap_err());
    assert!(error.contains("BPS recipe id mismatch"));
}

#[test]
fn package_rejects_a_source_that_does_not_match_the_recipe() {
    let (mut source, recipe, target) = fixture_product();
    let recipe_json = serde_json::to_string_pretty(&recipe).unwrap();
    let package = create_patch_package(&recipe_json, &source, &target).unwrap();
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
    let package = write_package_entries(&[
        (RECIPE_ENTRY_NAME, &oversized_recipe),
        (BPS_ENTRY_NAME, b"BPS1"),
    ]);

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
