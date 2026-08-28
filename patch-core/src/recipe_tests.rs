use super::*;

fn valid_recipe() -> PatchRecipe {
    PatchRecipe {
        format: PACKAGE_FORMAT.to_owned(),
        id: "fixture-patch".to_owned(),
        title: "Fixture Patch".to_owned(),
        output_filename: "fixture-patched.hdm".to_owned(),
        source: SourceImage {
            size: 1_474_560,
            sha256: "0".repeat(64),
            geometry: Fat12Geometry {
                bytes_per_sector: 512,
                sectors_per_cluster: 1,
                reserved_sectors: 1,
                fat_count: 2,
                root_entries: 224,
                total_sectors: 2_880,
                media_descriptor: 0xf0,
                sectors_per_fat: 9,
                sectors_per_track: 18,
                heads: 2,
            },
            mount_policy: MountPolicy::Standard,
        },
        assembly: AssemblyRecipe {
            retained_files: vec![ExactFile {
                name: "SYSTEM.SYS".to_owned(),
                size: 4,
                sha256: "1".repeat(64),
            }],
            placed_files: vec![PlacedFile {
                name: "GAME.COM".to_owned(),
                source: FileSource::RootFile {
                    name: "INSTALL.BIN".to_owned(),
                },
                source_size: 8,
                source_sha256: "2".repeat(64),
                transform: FileTransform::Bps {
                    target_size: 12,
                    target_sha256: "3".repeat(64),
                },
            }],
        },
        target: TargetImage {
            size: 1_474_560,
            sha256: "4".repeat(64),
        },
    }
}

fn valid_plan() -> PatchPlan {
    let recipe = valid_recipe();
    PatchPlan {
        id: recipe.id,
        title: recipe.title,
        output_filename: recipe.output_filename,
        source: recipe.source,
        assembly: PlannedAssemblyRecipe {
            retained_files: recipe.assembly.retained_files,
            placed_files: recipe
                .assembly
                .placed_files
                .into_iter()
                .map(|file| PlannedFile {
                    name: file.name,
                    source: file.source,
                    source_size: file.source_size,
                    source_sha256: file.source_sha256,
                    transform: PlannedTransform::Bps,
                })
                .collect(),
        },
    }
}

#[test]
fn plan_and_recipe_accept_complete_exact_file_contracts() {
    valid_plan().validate().unwrap();
    valid_recipe().validate().unwrap();
}

#[test]
fn recipe_rejects_duplicate_output_names() {
    let mut recipe = valid_recipe();
    recipe.assembly.placed_files[0].name = "SYSTEM.SYS".to_owned();
    let error = recipe.validate().unwrap_err().to_string();
    assert!(error.contains("duplicate output file name"));
}

#[test]
fn recipe_rejects_unknown_json_fields_and_another_format() {
    let mut value = serde_json::to_value(valid_recipe()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("revision".to_owned(), 1.into());
    assert!(parse_recipe(&value.to_string()).is_err());

    let mut recipe = valid_recipe();
    recipe.format = "another-format".to_owned();
    assert!(
        recipe
            .validate()
            .unwrap_err()
            .to_string()
            .contains("format")
    );
}

#[test]
fn recipe_rejects_noncanonical_dos_names() {
    let mut recipe = valid_recipe();
    recipe.assembly.placed_files[0].name = "game.com".to_owned();
    let error = recipe.validate().unwrap_err().to_string();
    assert!(error.contains("uppercase DOS 8.3"));
}

#[test]
fn empty_source_requires_the_canonical_zero_byte_identity() {
    let mut recipe = valid_recipe();
    let file = &mut recipe.assembly.placed_files[0];
    file.source = FileSource::Empty;
    file.source_size = 0;
    file.source_sha256 = crate::hash::EMPTY_SHA256.to_owned();
    recipe.validate().unwrap();

    let mut wrong_size = recipe.clone();
    wrong_size.assembly.placed_files[0].source_size = 1;
    assert!(
        wrong_size
            .validate()
            .unwrap_err()
            .to_string()
            .contains("must have size 0")
    );

    let mut wrong_hash = recipe;
    wrong_hash.assembly.placed_files[0].source_sha256 = "0".repeat(64);
    assert!(
        wrong_hash
            .validate()
            .unwrap_err()
            .to_string()
            .contains("SHA-256 of zero bytes")
    );
}

#[test]
fn recipe_rejects_an_hdm_larger_than_the_application_budget() {
    let mut recipe = valid_recipe();
    recipe.source.size = MAX_HDM_BYTES + 1;
    recipe.target.size = recipe.source.size;
    let error = recipe.validate().unwrap_err().to_string();
    assert!(error.contains("source HDM is too large"));
}

#[test]
fn recipe_rejects_a_source_or_target_file_larger_than_its_hdm() {
    let mut recipe = valid_recipe();
    recipe.assembly.placed_files[0].source_size = recipe.source.size + 1;
    assert!(
        recipe
            .validate()
            .unwrap_err()
            .to_string()
            .contains("source file GAME.COM is larger")
    );

    let mut recipe = valid_recipe();
    let FileTransform::Bps { target_size, .. } = &mut recipe.assembly.placed_files[0].transform
    else {
        unreachable!()
    };
    *target_size = recipe.source.size + 1;
    assert!(
        recipe
            .validate()
            .unwrap_err()
            .to_string()
            .contains("target file GAME.COM is larger")
    );
}

#[test]
fn recipe_rejects_more_output_files_than_the_root_directory_can_hold() {
    let mut recipe = valid_recipe();
    recipe.source.geometry.root_entries = 1;
    let error = recipe.validate().unwrap_err().to_string();
    assert!(error.contains("root files but geometry has 1 root entries"));
}

#[test]
fn recipe_rejects_more_output_file_bytes_than_the_disk_can_hold() {
    let mut recipe = valid_recipe();
    recipe.assembly.retained_files[0].size = recipe.source.size;
    let error = recipe.validate().unwrap_err().to_string();
    assert!(error.contains("bytes of root files but the source HDM is"));
}
