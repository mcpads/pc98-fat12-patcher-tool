use super::*;
use crate::test_support::{direct_root_plan, fixture_image};

#[test]
fn direct_root_source_is_selected_by_name_size_and_hash() {
    let retained = b"system";
    let payload = b"game payload";
    let source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], false);
    let plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let resolved = resolve_plan_files(&source, &plan).unwrap();
    assert_eq!(resolved["GAME.COM"], payload);
}

#[test]
fn direct_root_source_with_wrong_hash_is_rejected() {
    let retained = b"system";
    let payload = b"game payload";
    let source = fixture_image(&[("SYSTEM.SYS", retained), ("INSTALL.BIN", payload)], false);
    let mut plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    plan.assembly.placed_files[0].source_sha256 = "f".repeat(64);
    let error = resolve_plan_files(&source, &plan).unwrap_err().to_string();
    assert!(error.contains("GAME.COM source SHA-256 mismatch"));
}

#[test]
fn invalid_mz_lha_container_is_rejected_as_an_archive() {
    let retained = b"system";
    let executable = b"not an mz archive";
    let source = fixture_image(
        &[("SYSTEM.SYS", retained), ("INSTALL.EXE", executable)],
        false,
    );
    let mut plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.EXE",
        "GAME.COM",
        b"member",
    );
    plan.assembly.placed_files[0].source = FileSource::MzLhaMember {
        container: "INSTALL.EXE".to_owned(),
        member: "GAME.COM".to_owned(),
    };
    let error = resolve_plan_files(&source, &plan).unwrap_err().to_string();
    assert!(error.contains("not an MZ executable"));
}

#[test]
fn plan_cannot_assign_two_sizes_to_the_same_lha_member() {
    let retained = b"system";
    let payload = b"member";
    let source = fixture_image(
        &[("SYSTEM.SYS", retained), ("INSTALL.EXE", b"container")],
        false,
    );
    let mut plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.EXE",
        "GAME.COM",
        payload,
    );
    plan.assembly.placed_files[0].source = FileSource::MzLhaMember {
        container: "INSTALL.EXE".to_owned(),
        member: "GAME.COM".to_owned(),
    };
    let mut second = plan.assembly.placed_files[0].clone();
    second.name = "GAME2.COM".to_owned();
    second.source_size += 1;
    plan.assembly.placed_files.push(second);

    let error = required_archive_members(&plan.assembly.placed_files)
        .unwrap_err()
        .to_string();
    assert!(error.contains("conflicting expected sizes"));
}
