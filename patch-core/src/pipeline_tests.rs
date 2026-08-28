use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use fatfs::{FileSystem, FsOptions};

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
fn complete_pipeline_rebuilds_baseline_and_applies_bps_exactly() {
    let (source, recipe, target) = fixture_product();
    let patch = create_recipe_patch(&recipe, &source, &target).unwrap();
    let baseline = build_baseline(&recipe, &source).unwrap();
    let applied = apply_recipe_patch(&recipe, &baseline, &patch).unwrap();
    assert_eq!(applied, target);
}

#[test]
fn pipeline_rejects_another_source_before_file_collection() {
    let (mut source, recipe, target) = fixture_product();
    let baseline = build_baseline(&recipe, &source).unwrap();
    let patch = bps::create_patch(&baseline, &target, &encode_metadata(&recipe).unwrap()).unwrap();
    let last = source.len() - 1;
    source[last] ^= 1;
    let error = build_baseline(&recipe, &source).unwrap_err().to_string();
    assert!(error.contains("source image SHA-256 mismatch"));
    assert!(apply_recipe_patch(&recipe, &baseline, &patch).is_ok());
}

#[test]
fn patch_for_another_recipe_is_rejected_even_when_bytes_fit() {
    let (source, recipe, target) = fixture_product();
    let baseline = build_baseline(&recipe, &source).unwrap();
    let mut other = recipe.clone();
    other.id = "another-patch".to_owned();
    let patch = bps::create_patch(&baseline, &target, &encode_metadata(&other).unwrap()).unwrap();
    let error = apply_recipe_patch(&recipe, &baseline, &patch)
        .unwrap_err()
        .to_string();
    assert!(error.contains("BPS recipe id mismatch"));
}

#[test]
fn recipe_author_cannot_pin_a_target_with_divergent_fat_mirrors() {
    let (source, mut recipe, mut target) = fixture_product();
    let geometry = &recipe.source.geometry;
    let first_fat = usize::from(geometry.bytes_per_sector) * usize::from(geometry.reserved_sectors);
    let second_fat =
        first_fat + usize::from(geometry.bytes_per_sector) * usize::from(geometry.sectors_per_fat);
    target[second_fat + 8] ^= 1;
    recipe.target.sha256 = sha256_hex(&target);

    let error = create_recipe_patch(&recipe, &source, &target)
        .unwrap_err()
        .to_string();
    assert!(error.contains("FAT mirror 1 differs"));
}
