use super::*;
use crate::fat_name::FatShortName;
use crate::hash::sha256_hex;
use crate::recipe::{
    AssemblyRecipe, Fat12Geometry, FileSource, FileTransform, LEGACY_PACKAGE_FORMAT, MountPolicy,
    PACKAGE_FORMAT, PatchRecipe, PlacedFile, SourceImage, TargetImage,
};

fn patched_file(source: &[u8], target: &[u8]) -> PlacedFile {
    PlacedFile {
        patch_key: None,
        name: FatShortName::ascii("GAME.COM"),
        source: FileSource::RootFile {
            name: FatShortName::ascii("GAME.COM"),
        },
        source_size: source.len(),
        source_sha256: sha256_hex(source),
        transform: FileTransform::Bps {
            target_size: target.len(),
            target_sha256: sha256_hex(target),
        },
    }
}

fn legacy_recipe(file: PlacedFile) -> PatchRecipe {
    PatchRecipe {
        format: LEGACY_PACKAGE_FORMAT.to_owned(),
        id: "fixture".to_owned(),
        title: "Fixture".to_owned(),
        output_filename: "fixture.hdm".to_owned(),
        source: SourceImage {
            size: 32_768,
            sha256: "0".repeat(64),
            geometry: Fat12Geometry {
                bytes_per_sector: 512,
                sectors_per_cluster: 1,
                reserved_sectors: 1,
                fat_count: 2,
                root_entries: 16,
                total_sectors: 64,
                media_descriptor: 0xf0,
                sectors_per_fat: 1,
                sectors_per_track: 8,
                heads: 2,
            },
            mount_policy: MountPolicy::Standard,
        },
        assembly: AssemblyRecipe {
            retained_files: Vec::new(),
            placed_files: vec![file],
        },
        target: TargetImage {
            size: 32_768,
            sha256: "1".repeat(64),
        },
    }
}

#[test]
fn file_patch_reproduces_the_declared_logical_file() {
    let source = b"original game bytes";
    let target = b"localized game bytes that may grow";
    let file = patched_file(source, target);
    let recipe = legacy_recipe(file.clone());
    let patch = create_file_patch(&recipe, &file, source, target).unwrap();

    assert_eq!(
        apply_file_patch(&recipe, &file, source, &patch).unwrap(),
        target
    );
}

#[test]
fn file_patch_is_bound_to_recipe_and_output_name() {
    let source = b"original";
    let target = b"localized";
    let file = patched_file(source, target);
    let recipe = legacy_recipe(file.clone());
    let patch = create_file_patch(&recipe, &file, source, target).unwrap();

    let mut another_recipe = recipe.clone();
    another_recipe.id = "another-recipe".to_owned();
    assert!(
        inspect_file_patch(&another_recipe, &file, &patch)
            .unwrap_err()
            .to_string()
            .contains("metadata")
    );
    let mut renamed = file;
    renamed.name = FatShortName::ascii("OTHER.COM");
    assert!(
        inspect_file_patch(&recipe, &renamed, &patch)
            .unwrap_err()
            .to_string()
            .contains("metadata")
    );
}

#[test]
fn raw_sfn_file_patch_is_bound_to_patch_key_and_exact_name_bytes() {
    let source = b"original";
    let target = b"localized";
    let mut file = patched_file(source, target);
    file.patch_key = Some("DOCHO-DATA".to_owned());
    file.name = FatShortName::Raw {
        raw_sfn_hex: "93b9919088d995b7444154".to_owned(),
    };
    let mut recipe = legacy_recipe(file.clone());
    recipe.format = PACKAGE_FORMAT.to_owned();
    let patch = create_file_patch(&recipe, &file, source, target).unwrap();

    let mut another_key = file.clone();
    another_key.patch_key = Some("OTHER-DATA".to_owned());
    assert!(
        inspect_file_patch(&recipe, &another_key, &patch)
            .unwrap_err()
            .to_string()
            .contains("metadata")
    );

    let mut another_name = file;
    another_name.name = FatShortName::Raw {
        raw_sfn_hex: "93b9919088d995b7444155".to_owned(),
    };
    assert!(
        inspect_file_patch(&recipe, &another_name, &patch)
            .unwrap_err()
            .to_string()
            .contains("metadata")
    );
}
