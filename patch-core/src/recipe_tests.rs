use super::*;

fn valid_recipe() -> PatchRecipe {
    PatchRecipe {
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
            baseline_sha256: "1".repeat(64),
            retained_files: vec![ExactFile {
                name: "SYSTEM.SYS".to_owned(),
                size: 4,
                sha256: "2".repeat(64),
            }],
            placed_files: vec![PlacedFile {
                name: "GAME.COM".to_owned(),
                source: FileSource::RootFile {
                    name: "INSTALL.BIN".to_owned(),
                },
                size: 8,
                sha256: "3".repeat(64),
            }],
        },
        target: TargetImage {
            size: 1_474_560,
            sha256: "4".repeat(64),
        },
    }
}

#[test]
fn recipe_accepts_a_complete_exact_file_plan() {
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
fn recipe_rejects_unknown_json_fields() {
    let mut value = serde_json::to_value(valid_recipe()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("revision".to_owned(), 1.into());
    assert!(parse_recipe(&value.to_string()).is_err());
}

#[test]
fn recipe_rejects_noncanonical_dos_names() {
    let mut recipe = valid_recipe();
    recipe.assembly.placed_files[0].name = "game.com".to_owned();
    let error = recipe.validate().unwrap_err().to_string();
    assert!(error.contains("uppercase DOS 8.3"));
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
fn recipe_rejects_a_file_larger_than_its_source_hdm() {
    let mut recipe = valid_recipe();
    recipe.assembly.placed_files[0].size = recipe.source.size + 1;
    let error = recipe.validate().unwrap_err().to_string();
    assert!(error.contains("placed file GAME.COM is larger"));
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
