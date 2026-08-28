use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use anyhow::{Context, Result, ensure};
use fatfs::{FatType, FileSystem, FsOptions};

use crate::hash::require_sha256;
use crate::limits::MAX_FAT_DIRECTORY_DEPTH;
use crate::recipe::{ExactFile, Fat12Geometry, MountPolicy, PatchRecipe};

const HIDDEN_SECTORS_OFFSET: usize = 28;
const TOTAL_SECTORS_32_OFFSET: usize = 32;
const IBM_SIGNATURE_OFFSET: usize = 510;

pub(crate) fn read_root_files(
    image: &[u8],
    policy: MountPolicy,
    names: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mount_image = mount_copy(image, policy)?;
    let filesystem = FileSystem::new(Cursor::new(mount_image), FsOptions::new())
        .context("mount source FAT12 image")?;
    ensure!(
        matches!(filesystem.fat_type(), FatType::Fat12),
        "source filesystem is not FAT12"
    );
    let root = filesystem.root_dir();
    let mut files = BTreeMap::new();
    for name in names {
        files.insert(name.clone(), read_root_file(&root, name, image.len())?);
    }
    Ok(files)
}

pub(crate) fn assemble_baseline(
    source: &[u8],
    recipe: &PatchRecipe,
    placed_files: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
    require_geometry(source, &recipe.source.geometry)?;
    let reserved_len = reserved_region_len(&recipe.source.geometry)?;
    let mut image = mount_copy(source, recipe.source.mount_policy)?;
    {
        let filesystem = FileSystem::new(Cursor::new(image.as_mut_slice()), FsOptions::new())
            .context("mount working FAT12 image")?;
        ensure!(
            matches!(filesystem.fat_type(), FatType::Fat12),
            "working filesystem is not FAT12"
        );
        let root = filesystem.root_dir();
        let retained_names: BTreeSet<_> = recipe
            .assembly
            .retained_files
            .iter()
            .map(|file| file.name.as_str())
            .collect();
        remove_nonretained_entries(&root, &retained_names)?;
        for expected in &recipe.assembly.retained_files {
            verify_root_file(&root, expected)?;
        }
        for file in &recipe.assembly.placed_files {
            let bytes = placed_files
                .get(&file.name)
                .with_context(|| format!("resolved file set is missing {}", file.name))?;
            write_root_file(&root, &file.name, bytes)?;
        }
        drop(root);
        filesystem
            .unmount()
            .context("unmount assembled FAT12 image")?;
    }
    image[..reserved_len].copy_from_slice(&source[..reserved_len]);
    verify_baseline(&image, recipe, placed_files)?;
    Ok(image)
}

pub(crate) fn require_geometry(image: &[u8], expected: &Fat12Geometry) -> Result<()> {
    let observed = parse_geometry(image)?;
    ensure!(
        observed == *expected,
        "source FAT12 geometry differs: expected {expected:?}, got {observed:?}"
    );
    Ok(())
}

pub(crate) fn require_fat12_structure(
    image: &[u8],
    expected: &Fat12Geometry,
    policy: MountPolicy,
) -> Result<()> {
    require_geometry(image, expected)?;
    verify_fat_mirrors(image, expected)?;
    let filesystem = FileSystem::new(Cursor::new(mount_copy(image, policy)?), FsOptions::new())
        .context("mount target FAT12 image")?;
    ensure!(
        matches!(filesystem.fat_type(), FatType::Fat12),
        "target filesystem is not FAT12"
    );
    for entry in filesystem.root_dir().iter() {
        entry.context("enumerate target FAT12 root")?;
    }
    Ok(())
}

fn parse_geometry(image: &[u8]) -> Result<Fat12Geometry> {
    ensure!(image.len() >= 28, "image is too short for a FAT12 BPB");
    let u16_at = |offset: usize| -> Result<u16> {
        let bytes: [u8; 2] = image
            .get(offset..offset + 2)
            .context("truncated FAT12 BPB")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("truncated FAT12 BPB field"))?;
        Ok(u16::from_le_bytes(bytes))
    };
    Ok(Fat12Geometry {
        bytes_per_sector: u16_at(11)?,
        sectors_per_cluster: image[13],
        reserved_sectors: u16_at(14)?,
        fat_count: image[16],
        root_entries: u16_at(17)?,
        total_sectors: u16_at(19)?,
        media_descriptor: image[21],
        sectors_per_fat: u16_at(22)?,
        sectors_per_track: u16_at(24)?,
        heads: u16_at(26)?,
    })
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

fn remove_nonretained_entries<T: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<'_, T>,
    retained: &BTreeSet<&str>,
) -> Result<()> {
    let entries = root
        .iter()
        .map(|entry| {
            entry.map(|entry| {
                (
                    entry.file_name(),
                    entry.is_dir(),
                    entry.file_name().to_ascii_uppercase(),
                )
            })
        })
        .collect::<std::io::Result<Vec<_>>>()
        .context("enumerate source FAT12 root")?;
    for (name, is_directory, upper_name) in entries {
        if retained.contains(upper_name.as_str()) {
            ensure!(!is_directory, "retained root entry is a directory: {name}");
            continue;
        }
        if is_directory {
            let directory = root
                .open_dir(&name)
                .with_context(|| format!("open root directory {name}"))?;
            remove_directory_contents(&directory, 1)?;
            drop(directory);
        }
        root.remove(&name)
            .with_context(|| format!("remove source root entry {name}"))?;
    }
    Ok(())
}

fn remove_directory_contents<T: fatfs::ReadWriteSeek>(
    directory: &fatfs::Dir<'_, T>,
    depth: usize,
) -> Result<()> {
    ensure!(
        depth <= MAX_FAT_DIRECTORY_DEPTH,
        "FAT12 directory nesting exceeds {MAX_FAT_DIRECTORY_DEPTH} levels"
    );
    let entries = directory
        .iter()
        .map(|entry| entry.map(|entry| (entry.file_name(), entry.is_dir())))
        .collect::<std::io::Result<Vec<_>>>()
        .context("enumerate FAT12 directory")?;
    for (name, is_directory) in entries {
        if matches!(name.as_str(), "." | "..") {
            continue;
        }
        if is_directory {
            let child = directory
                .open_dir(&name)
                .with_context(|| format!("open FAT12 directory {name}"))?;
            let child_depth = depth
                .checked_add(1)
                .context("FAT12 directory depth overflow")?;
            remove_directory_contents(&child, child_depth)?;
            drop(child);
        }
        directory
            .remove(&name)
            .with_context(|| format!("remove FAT12 entry {name}"))?;
    }
    Ok(())
}

fn write_root_file<T: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<'_, T>,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    let mut file = root
        .create_file(name)
        .with_context(|| format!("create FAT12 file {name}"))?;
    file.truncate()
        .with_context(|| format!("truncate FAT12 file {name}"))?;
    file.write_all(bytes)
        .with_context(|| format!("write FAT12 file {name}"))?;
    Ok(())
}

fn read_root_file<T: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<'_, T>,
    name: &str,
    maximum_size: usize,
) -> Result<Vec<u8>> {
    let mut file = root
        .open_file(name)
        .with_context(|| format!("required FAT12 root file is missing: {name}"))?;
    let announced_size = file
        .seek(SeekFrom::End(0))
        .with_context(|| format!("read FAT12 file size {name}"))?;
    ensure!(
        announced_size <= maximum_size as u64,
        "FAT12 file {name} is too large: {announced_size} bytes exceeds {maximum_size}"
    );
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("rewind FAT12 file {name}"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(announced_size as usize)
        .with_context(|| format!("reserve FAT12 file {name} buffer"))?;
    let read_limit = u64::try_from(maximum_size)
        .context("FAT12 file size limit does not fit reader")?
        .checked_add(1)
        .context("FAT12 file read limit overflow")?;
    (&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read FAT12 file {name}"))?;
    ensure!(
        bytes.len() as u64 == announced_size,
        "FAT12 file {name} announced {announced_size} bytes but yielded {}",
        bytes.len()
    );
    Ok(bytes)
}

fn verify_root_file<T: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<'_, T>,
    expected: &ExactFile,
) -> Result<()> {
    let bytes = read_root_file(root, &expected.name, expected.size)?;
    ensure!(
        bytes.len() == expected.size,
        "{} size mismatch: expected {}, got {}",
        expected.name,
        expected.size,
        bytes.len()
    );
    require_sha256(&bytes, &expected.sha256, &expected.name)
}

fn verify_baseline(
    image: &[u8],
    recipe: &PatchRecipe,
    placed_files: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    ensure!(
        image.len() == recipe.source.size,
        "assembled image size changed: expected {}, got {}",
        recipe.source.size,
        image.len()
    );
    require_geometry(image, &recipe.source.geometry)?;
    verify_fat_mirrors(image, &recipe.source.geometry)?;
    let filesystem = FileSystem::new(
        Cursor::new(mount_copy(image, recipe.source.mount_policy)?),
        FsOptions::new(),
    )
    .context("remount assembled FAT12 image")?;
    let root = filesystem.root_dir();
    let mut actual_names = BTreeSet::new();
    for entry in root.iter() {
        let entry = entry.context("enumerate assembled FAT12 root")?;
        ensure!(
            !entry.is_dir(),
            "assembled root contains an unexpected directory: {}",
            entry.file_name()
        );
        actual_names.insert(entry.file_name().to_ascii_uppercase());
    }
    let expected_names: BTreeSet<_> = recipe
        .assembly
        .retained_files
        .iter()
        .map(|file| file.name.clone())
        .chain(
            recipe
                .assembly
                .placed_files
                .iter()
                .map(|file| file.name.clone()),
        )
        .collect();
    ensure!(
        actual_names == expected_names,
        "assembled root file set differs: expected {expected_names:?}, got {actual_names:?}"
    );
    for file in &recipe.assembly.retained_files {
        verify_root_file(&root, file)?;
    }
    for file in &recipe.assembly.placed_files {
        let expected = placed_files
            .get(&file.name)
            .with_context(|| format!("resolved file set is missing {}", file.name))?;
        let actual = read_root_file(&root, &file.name, expected.len())?;
        ensure!(actual == *expected, "assembled file differs: {}", file.name);
    }
    Ok(())
}

fn verify_fat_mirrors(image: &[u8], geometry: &Fat12Geometry) -> Result<()> {
    if geometry.fat_count == 1 {
        return Ok(());
    }
    let bytes_per_sector = usize::from(geometry.bytes_per_sector);
    let fat_size = bytes_per_sector
        .checked_mul(usize::from(geometry.sectors_per_fat))
        .context("FAT byte size overflow")?;
    let first_offset = bytes_per_sector
        .checked_mul(usize::from(geometry.reserved_sectors))
        .context("FAT offset overflow")?;
    let first = image
        .get(first_offset..first_offset + fat_size)
        .context("first FAT lies outside image")?;
    for index in 1..usize::from(geometry.fat_count) {
        let offset = first_offset + index * fat_size;
        let mirror = image
            .get(offset..offset + fat_size)
            .context("FAT mirror lies outside image")?;
        ensure!(
            mirror == first,
            "FAT mirror {index} differs from the first FAT"
        );
    }
    Ok(())
}

fn reserved_region_len(geometry: &Fat12Geometry) -> Result<usize> {
    usize::from(geometry.bytes_per_sector)
        .checked_mul(usize::from(geometry.reserved_sectors))
        .context("reserved FAT12 region size overflow")
}

#[cfg(test)]
#[path = "fat12_tests.rs"]
mod fat12_tests;
