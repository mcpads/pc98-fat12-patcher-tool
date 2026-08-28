use super::*;
use crate::test_support::{direct_root_recipe, fixture_image};

#[test]
fn direct_root_source_is_selected_by_name_size_and_hash() {
    let retained = b"system";
    let payload = b"game payload";
    let source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], false);
    let recipe = direct_root_recipe(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let resolved = resolve_assembly_files(&source, &recipe).unwrap();
    assert_eq!(resolved["GAME.COM"], payload);
}

#[test]
fn direct_root_source_with_wrong_hash_is_rejected() {
    let retained = b"system";
    let payload = b"game payload";
    let source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], false);
    let mut recipe = direct_root_recipe(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    recipe.assembly.placed_files[0].sha256 = "f".repeat(64);
    let error = resolve_assembly_files(&source, &recipe)
        .unwrap_err()
        .to_string();
    assert!(error.contains("GAME.COM SHA-256 mismatch"));
}

#[test]
fn invalid_mz_lha_container_is_rejected_as_an_archive() {
    let retained = b"system";
    let executable = b"not an mz archive";
    let source = fixture_image(
        &[("SYSTEM.SYS", retained), ("INSTALL.EXE", executable)],
        false,
    );
    let mut recipe = direct_root_recipe(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.EXE",
        "GAME.COM",
        b"member",
    );
    recipe.assembly.placed_files[0].source = FileSource::MzLhaMember {
        container: "INSTALL.EXE".to_owned(),
        member: "GAME.COM".to_owned(),
    };
    let error = resolve_assembly_files(&source, &recipe)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not an MZ executable"));
}

#[test]
fn recipe_cannot_assign_two_sizes_to_the_same_lha_member() {
    let retained = b"system";
    let payload = b"member";
    let source = fixture_image(
        &[("SYSTEM.SYS", retained), ("INSTALL.EXE", b"container")],
        false,
    );
    let mut recipe = direct_root_recipe(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.EXE",
        "GAME.COM",
        payload,
    );
    recipe.assembly.placed_files[0].source = FileSource::MzLhaMember {
        container: "INSTALL.EXE".to_owned(),
        member: "GAME.COM".to_owned(),
    };
    let mut second = recipe.assembly.placed_files[0].clone();
    second.name = "GAME2.COM".to_owned();
    second.size += 1;
    recipe.assembly.placed_files.push(second);

    let error = required_archive_members(&recipe).unwrap_err().to_string();
    assert!(error.contains("conflicting expected sizes"));
}
