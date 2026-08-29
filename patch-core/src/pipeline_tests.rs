use super::*;
use crate::recipe::{FileSource, PlannedTransform};
use crate::test_support::{content_image, direct_root_plan, fixture_image};

fn fixture_product() -> (Vec<u8>, PatchPlan, Vec<u8>) {
    let retained = b"system";
    let payload = b"original game payload";
    let localized = b"localized game payload that grows";
    let source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], false);
    let plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let mut content = content_image(&source, &plan, &[("GAME.COM", localized)]);
    let last = content.len() - 1;
    content[last] = 0x7b;
    (source, plan, content)
}

#[test]
fn complete_pipeline_patches_logical_files_and_rebuilds_the_image() {
    let (source, plan, content) = fixture_product();
    let created = create_package_contents(plan, &source, &content).unwrap();
    let applied = apply_package_contents(&created.recipe, &created.patches, &source).unwrap();

    assert_eq!(applied, created.target);
    assert_ne!(
        applied, content,
        "the canonical image need not preserve donor slack"
    );
}

#[test]
fn pipeline_rejects_an_assembled_image_with_another_target_identity() {
    let (source, plan, content) = fixture_product();
    let created = create_package_contents(plan, &source, &content).unwrap();
    let mut recipe = created.recipe;
    recipe.target.sha256 = "0".repeat(64);

    let error = apply_package_contents(&recipe, &created.patches, &source)
        .expect_err("another target identity must not produce a result")
        .to_string();

    assert!(error.contains("target image SHA-256 mismatch"));
}

#[test]
fn complete_pipeline_creates_a_new_file_from_an_empty_bps_source() {
    let retained = b"system";
    let generated_font = b"generated Korean font records";
    let source = fixture_image(&[("SYSTEM.SYS", retained)], false);
    let mut plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "UNUSED.BIN",
        "KIKIKR.FNT",
        b"",
    );
    plan.assembly.placed_files[0].source = FileSource::Empty;
    let content = content_image(&source, &plan, &[("KIKIKR.FNT", generated_font)]);

    let created = create_package_contents(plan, &source, &content).unwrap();
    let applied = apply_package_contents(&created.recipe, &created.patches, &source).unwrap();

    assert_eq!(applied, created.target);
    assert!(created.patches.contains_key("KIKIKR.FNT"));
    let font_name = crate::fat_name::FatShortName::ascii("KIKIKR.FNT")
        .raw_bytes("font")
        .unwrap();
    let files = crate::fat12::read_root_files(
        &applied,
        crate::recipe::MountPolicy::Standard,
        &BTreeSet::from([font_name]),
    )
    .unwrap();
    assert_eq!(files[&font_name], generated_font);
}

#[test]
fn pipeline_rejects_another_source_before_file_collection() {
    let (mut source, plan, content) = fixture_product();
    let last = source.len() - 1;
    source[last] ^= 1;
    let error = create_package_contents(plan, &source, &content)
        .err()
        .expect("wrong source must fail")
        .to_string();
    assert!(error.contains("source image SHA-256 mismatch"));
}

#[test]
fn copy_transform_rejects_changed_content() {
    let (source, mut plan, content) = fixture_product();
    plan.assembly.placed_files[0].transform = PlannedTransform::Copy;
    let error = create_package_contents(plan, &source, &content)
        .err()
        .expect("changed copy input must fail")
        .to_string();
    assert!(error.contains("declared copy but content image changes it"));
}

#[test]
fn content_image_with_divergent_fat_mirrors_is_rejected() {
    let (source, plan, mut content) = fixture_product();
    let geometry = &plan.source.geometry;
    let first_fat = usize::from(geometry.bytes_per_sector) * usize::from(geometry.reserved_sectors);
    let second_fat =
        first_fat + usize::from(geometry.bytes_per_sector) * usize::from(geometry.sectors_per_fat);
    content[second_fat + 8] ^= 1;

    let error = create_package_contents(plan, &source, &content)
        .err()
        .expect("divergent FAT mirror must fail")
        .to_string();
    assert!(error.contains("FAT mirror 1 differs"));
}

#[test]
fn legacy_ascii_package_reproduces_deleted_directory_metadata() {
    let retained = b"system";
    let payload = b"original game payload";
    let localized = b"localized game payload";
    let mut source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], true);
    let geometry = crate::test_support::fixture_geometry();
    let root_start = usize::from(geometry.bytes_per_sector)
        * (usize::from(geometry.reserved_sectors)
            + usize::from(geometry.fat_count) * usize::from(geometry.sectors_per_fat));
    let directory_offset = (0..usize::from(geometry.root_entries))
        .map(|index| root_start + index * 32)
        .find(|offset| source[*offset..*offset + 11] == *b"JUNK       ")
        .expect("fixture directory entry");
    source[directory_offset + 22..directory_offset + 26].copy_from_slice(&[1, 2, 3, 4]);

    let plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let content = content_image(&source, &plan, &[("GAME.COM", localized)]);
    let created = create_package_contents(plan, &source, &content).unwrap();
    let applied = apply_package_contents(&created.recipe, &created.patches, &source).unwrap();

    assert_eq!(applied[directory_offset], 0xe5);
    assert_eq!(
        &applied[directory_offset + 22..directory_offset + 26],
        &[0; 4],
        "existing ASCII packages depend on the original fatfs deletion bytes"
    );
}
