use std::collections::BTreeMap;

use fatfs::{FileSystem, FsOptions};

use super::*;
use crate::test_support::{direct_root_recipe, fixture_image};

#[test]
fn assembly_preserves_declared_files_and_replaces_the_rest_in_order() {
    let retained = b"system";
    let payload = b"game payload";
    let source = fixture_image(
        &[
            ("SYSTEM.SYS", retained),
            ("INSTALL.BIN", payload),
            ("REMOVE.TXT", b"obsolete"),
        ],
        true,
    );
    let recipe = direct_root_recipe(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let placed = BTreeMap::from([("GAME.COM".to_owned(), payload.to_vec())]);
    let assembled = assemble_baseline(&source, &recipe, &placed).unwrap();

    assert_eq!(source.len(), assembled.len());
    assert_eq!(&source[..512], &assembled[..512]);
    let filesystem = FileSystem::new(Cursor::new(assembled), FsOptions::new()).unwrap();
    let root = filesystem.root_dir();
    let names: BTreeSet<_> = root
        .iter()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["GAME.COM".to_owned(), "SYSTEM.SYS".to_owned()])
    );
    assert_eq!(
        read_root_file(&root, "SYSTEM.SYS", retained.len()).unwrap(),
        retained
    );
    assert_eq!(
        read_root_file(&root, "GAME.COM", payload.len()).unwrap(),
        payload
    );
}

#[test]
fn pc98_dos3_mount_copy_does_not_change_the_caller_source() {
    let mut source = vec![0xe5; 1_024];
    let original = source.clone();
    let copy = mount_copy(&source, MountPolicy::Pc98Dos3).unwrap();
    assert_eq!(source, original);
    assert_eq!(
        &copy[HIDDEN_SECTORS_OFFSET..TOTAL_SECTORS_32_OFFSET + 4],
        &[0; 8]
    );
    assert_eq!(
        &copy[IBM_SIGNATURE_OFFSET..IBM_SIGNATURE_OFFSET + 2],
        &[0x55, 0xaa]
    );
    source[0] = 0;
    assert_ne!(source, original);
}

#[test]
fn geometry_mismatch_is_rejected_before_reallocation() {
    let image = fixture_image(&[("ONE.TXT", b"one")], false);
    let mut geometry = crate::test_support::fixture_geometry();
    geometry.sectors_per_track = 8;
    let error = require_geometry(&image, &geometry).unwrap_err().to_string();
    assert!(error.contains("geometry differs"));
}
