use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use anyhow::{Context, Result, ensure};
use fatfs::{DirEntry, FatType, FileSystem, FsOptions, ReadWriteSeek};

use super::{FilePlacement, require_geometry, verify_image};
use crate::limits::MAX_FAT_DIRECTORY_DEPTH;
use crate::recipe::{ExactFile, MountPolicy, SourceImage};

const HIDDEN_SECTORS_OFFSET: usize = 28;
const TOTAL_SECTORS_32_OFFSET: usize = 32;
const IBM_SIGNATURE_OFFSET: usize = 510;

pub(crate) fn assemble_legacy_ascii_image(
    source: &[u8],
    source_profile: &SourceImage,
    retained_files: &[ExactFile],
    placed_files: &[FilePlacement],
    placed_file_bytes: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
    require_geometry(source, &source_profile.geometry)?;
    let reserved_len = usize::from(source_profile.geometry.bytes_per_sector)
        .checked_mul(usize::from(source_profile.geometry.reserved_sectors))
        .context("reserved FAT12 region size overflow")?;
    let mut image = mount_copy(source, source_profile.mount_policy)?;
    {
        let filesystem = FileSystem::new(Cursor::new(image.as_mut_slice()), FsOptions::new())
            .context("mount legacy ASCII FAT12 image")?;
        ensure!(
            matches!(filesystem.fat_type(), FatType::Fat12),
            "legacy ASCII source filesystem is not FAT12"
        );
        let root = filesystem.root_dir();
        let retained_names = retained_files
            .iter()
            .map(|file| {
                file.name
                    .ascii_name()
                    .context("legacy package retained file is not ASCII")
            })
            .collect::<Result<BTreeSet<_>>>()?;
        remove_nonretained_entries(&root, &retained_names)?;
        for placement in placed_files {
            let name = placement
                .name
                .ascii_name()
                .context("legacy package output file is not ASCII")?;
            let bytes = placed_file_bytes
                .get(&placement.patch_key)
                .with_context(|| format!("resolved file set is missing {}", placement.patch_key))?;
            write_root_file(&root, name, bytes)?;
        }
        drop(root);
        filesystem
            .unmount()
            .context("unmount legacy ASCII FAT12 image")?;
    }
    image[..reserved_len].copy_from_slice(&source[..reserved_len]);
    verify_image(
        &image,
        source_profile,
        retained_files,
        placed_files,
        placed_file_bytes,
    )?;
    Ok(image)
}

fn mount_copy(image: &[u8], policy: MountPolicy) -> Result<Vec<u8>> {
    let mut copy = image.to_vec();
    if policy == MountPolicy::Pc98Dos3 {
        ensure!(
            copy.len() > IBM_SIGNATURE_OFFSET + 1,
            "image is too short for PC-98 DOS 3 FAT compatibility fields"
        );
        copy[HIDDEN_SECTORS_OFFSET..TOTAL_SECTORS_32_OFFSET + 4].fill(0);
        copy[IBM_SIGNATURE_OFFSET..IBM_SIGNATURE_OFFSET + 2].copy_from_slice(&[0x55, 0xaa]);
    }
    Ok(copy)
}

fn remove_nonretained_entries<T: ReadWriteSeek>(
    root: &fatfs::Dir<'_, T>,
    retained: &BTreeSet<&str>,
) -> Result<()> {
    let mut entries = Vec::new();
    for entry in root.iter() {
        let entry = entry.context("enumerate legacy ASCII source root")?;
        entries.push((short_file_name(&entry)?, entry.is_dir()));
    }
    for (name, is_directory) in entries {
        if retained.contains(name.as_str()) {
            ensure!(!is_directory, "retained root entry is a directory: {name}");
            continue;
        }
        if is_directory {
            let directory = root
                .open_dir(&name)
                .with_context(|| format!("open legacy ASCII root directory {name}"))?;
            remove_directory_contents(&directory, 1)?;
            drop(directory);
        }
        root.remove(&name)
            .with_context(|| format!("remove legacy ASCII root entry {name}"))?;
    }
    Ok(())
}

fn remove_directory_contents<T: ReadWriteSeek>(
    directory: &fatfs::Dir<'_, T>,
    depth: usize,
) -> Result<()> {
    ensure!(
        depth <= MAX_FAT_DIRECTORY_DEPTH,
        "FAT12 directory nesting exceeds {MAX_FAT_DIRECTORY_DEPTH} levels"
    );
    let mut entries = Vec::new();
    for entry in directory.iter() {
        let entry = entry.context("enumerate legacy ASCII FAT12 directory")?;
        entries.push((short_file_name(&entry)?, entry.is_dir()));
    }
    for (name, is_directory) in entries {
        if matches!(name.as_str(), "." | "..") {
            continue;
        }
        if is_directory {
            let child = directory
                .open_dir(&name)
                .with_context(|| format!("open legacy ASCII FAT12 directory {name}"))?;
            remove_directory_contents(
                &child,
                depth
                    .checked_add(1)
                    .context("FAT12 directory depth overflow")?,
            )?;
            drop(child);
        }
        directory
            .remove(&name)
            .with_context(|| format!("remove legacy ASCII FAT12 entry {name}"))?;
    }
    Ok(())
}

fn write_root_file<T: ReadWriteSeek>(
    root: &fatfs::Dir<'_, T>,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    let mut file = root
        .create_file(name)
        .with_context(|| format!("create legacy ASCII FAT12 file {name}"))?;
    file.truncate()
        .with_context(|| format!("truncate legacy ASCII FAT12 file {name}"))?;
    file.write_all(bytes)
        .with_context(|| format!("write legacy ASCII FAT12 file {name}"))
}

fn short_file_name<T: ReadWriteSeek>(entry: &DirEntry<'_, T>) -> Result<String> {
    let bytes = entry.short_file_name_as_bytes();
    ensure!(
        bytes.is_ascii(),
        "legacy ASCII FAT12 name contains a non-ASCII OEM byte: {bytes:?}"
    );
    Ok(std::str::from_utf8(bytes)
        .context("legacy ASCII FAT12 name is not ASCII")?
        .to_ascii_uppercase())
}
