use std::collections::BTreeMap;

use fatfs::{FileSystem, FsOptions};

use super::*;
use crate::test_support::{direct_root_plan, fixture_image};

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
    let plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "INSTALL.BIN",
        "GAME.COM",
        payload,
    );
    let placed = BTreeMap::from([("GAME.COM".to_owned(), payload.to_vec())]);
    let placed_names = vec!["GAME.COM".to_owned()];
    let assembled = assemble_image(
        &source,
        &plan.source,
        &plan.assembly.retained_files,
        &placed_names,
        &placed,
    )
    .unwrap();

    assert_eq!(source.len(), assembled.len());
    assert_eq!(&source[..512], &assembled[..512]);
    let filesystem = FileSystem::new(Cursor::new(assembled), FsOptions::new()).unwrap();
    let root = filesystem.root_dir();
    let names: BTreeSet<_> = root
        .iter()
        .map(|entry| short_file_name(&entry.unwrap()).unwrap())
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
fn canonical_assembly_follows_placed_order_and_preserves_cluster_tail_bytes() {
    let retained = b"system";
    let old_first = vec![0xa5; 700];
    let old_second = vec![0xb6; 32];
    let source = fixture_image(
        &[
            ("SYSTEM.SYS", retained),
            ("OLD1.BIN", &old_first),
            ("OLD2.BIN", &old_second),
        ],
        false,
    );
    assert_eq!(
        crate::hash::sha256_hex(&source),
        "dde046d0b7826a723f2a8422529346433a48c2ddc22882be226dc3e140240b0c"
    );
    let plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        retained,
        "OLD1.BIN",
        "ZNEW.BIN",
        &old_first,
    );
    let first = vec![0x31; 600];
    let second = vec![0x42; 16];
    let placed_names = vec!["ZNEW.BIN".to_owned(), "ANEW.BIN".to_owned()];
    let placed = BTreeMap::from([
        ("ANEW.BIN".to_owned(), second),
        ("ZNEW.BIN".to_owned(), first.clone()),
    ]);

    let assembled = assemble_image(
        &source,
        &plan.source,
        &plan.assembly.retained_files,
        &placed_names,
        &placed,
    )
    .unwrap();

    let geometry = crate::test_support::fixture_geometry();
    let sector_size = usize::from(geometry.bytes_per_sector);
    let fat_size = sector_size * usize::from(geometry.sectors_per_fat);
    let root_start = sector_size * usize::from(geometry.reserved_sectors)
        + fat_size * usize::from(geometry.fat_count);
    let root_entry = |index: usize| {
        assembled
            .get(root_start + index * 32..root_start + (index + 1) * 32)
            .unwrap()
    };

    assert_eq!(&root_entry(1)[..11], b"ZNEW    BIN");
    assert_eq!(root_entry(1)[11], 0);
    assert_eq!(&root_entry(1)[12..26], &[0; 14]);
    assert_eq!(
        u16::from_le_bytes(root_entry(1)[26..28].try_into().unwrap()),
        3
    );
    assert_eq!(
        u32::from_le_bytes(root_entry(1)[28..32].try_into().unwrap()),
        600
    );
    assert_eq!(&root_entry(2)[..11], b"ANEW    BIN");
    assert_eq!(
        u16::from_le_bytes(root_entry(2)[26..28].try_into().unwrap()),
        5
    );
    assert_ne!(root_entry(1)[11], 0x0f, "placed files use one SFN entry");

    let fat_value = |fat_offset: usize, cluster: usize| {
        let offset = fat_offset + cluster + cluster / 2;
        let packed = u16::from_le_bytes(assembled[offset..offset + 2].try_into().unwrap());
        if cluster.is_multiple_of(2) {
            packed & 0x0fff
        } else {
            packed >> 4
        }
    };
    let first_fat = sector_size * usize::from(geometry.reserved_sectors);
    assert_eq!(fat_value(first_fat, 3), 4);
    assert_eq!(fat_value(first_fat, 4), 0x0fff);
    assert_eq!(fat_value(first_fat, 5), 0x0fff);
    assert_eq!(
        &assembled[first_fat..first_fat + fat_size],
        &assembled[first_fat + fat_size..first_fat + fat_size * 2]
    );

    let root_sectors = (usize::from(geometry.root_entries) * 32).div_ceil(sector_size);
    let data_start = root_start + root_sectors * sector_size;
    let cluster_size = sector_size * usize::from(geometry.sectors_per_cluster);
    let second_cluster_offset = data_start + (4 - 2) * cluster_size;
    let written_in_second_cluster = first.len() - cluster_size;
    assert_eq!(
        &assembled[second_cluster_offset + written_in_second_cluster
            ..second_cluster_offset + cluster_size],
        &source[second_cluster_offset + written_in_second_cluster
            ..second_cluster_offset + cluster_size],
        "bytes after the logical EOF remain from the exact source image"
    );
    assert_eq!(
        crate::hash::sha256_hex(&assembled),
        "568f66626134c105ef5f243f6eb86c2fa7a8697de78b157898a46cab1e489e2c"
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
