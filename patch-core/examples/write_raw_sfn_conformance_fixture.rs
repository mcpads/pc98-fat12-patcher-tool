use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, format_volume};
use pc98_fat12_patcher_core::{PACKAGE_FORMAT, apply_patch_package, create_patch_package};
use serde_json::json;
use sha2::{Digest, Sha256};

const BYTES_PER_SECTOR: usize = 512;
const TOTAL_SECTORS: usize = 64;
const ASCII_FIXTURE_SFN: &[u8; 11] = b"DOCHO   DAT";
const RAW_FIXTURE_SFN: &[u8; 11] = b"\x93\xb9\x91\x90\x88\xd9\x95\xb7DAT";
const RAW_FIXTURE_LHA_NAME: &[u8; 12] = b"\x93\xb9\x91\x90\x88\xd9\x95\xb7.DAT";

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let output_directory = std::env::args_os().nth(1).map(PathBuf::from).context(
        "usage: cargo run --example write_raw_sfn_conformance_fixture -- <output-directory>",
    )?;
    let source_path = output_directory.join("source.hdm");
    let plan_path = output_directory.join("plan.json");
    let package_path = output_directory.join("package.zip");
    let target_path = output_directory.join("target.hdm");
    let manifest_path = output_directory.join("manifest.json");
    for path in [
        &source_path,
        &plan_path,
        &package_path,
        &target_path,
        &manifest_path,
    ] {
        ensure!(
            !path.exists(),
            "refusing to overwrite conformance fixture {}",
            path.display()
        );
    }

    let retained = b"system";
    let original = vec![0x5a; 700];
    let localized = vec![0x31; 733];
    let source = make_source(retained, &original)?;
    let content = make_content(retained, &localized)?;
    let plan = json!({
        "format": PACKAGE_FORMAT,
        "id": "canonical-fat12-raw-sfn-conformance",
        "title": "Canonical FAT12 raw SFN conformance fixture",
        "output_filename": "raw-sfn-conformance-target.hdm",
        "source": {
            "size": source.len(),
            "sha256": sha256_hex(&source),
            "geometry": {
                "bytes_per_sector": u16_at(&source, 11)?,
                "sectors_per_cluster": source[13],
                "reserved_sectors": u16_at(&source, 14)?,
                "fat_count": source[16],
                "root_entries": u16_at(&source, 17)?,
                "total_sectors": u16_at(&source, 19)?,
                "media_descriptor": source[21],
                "sectors_per_fat": u16_at(&source, 22)?,
                "sectors_per_track": u16_at(&source, 24)?,
                "heads": u16_at(&source, 26)?
            },
            "mount_policy": "standard"
        },
        "assembly": {
            "retained_files": [{
                "name": "KEEP.SYS",
                "size": retained.len(),
                "sha256": sha256_hex(retained)
            }],
            "placed_files": [{
                "patch_key": "DOCHO-DATA",
                "name": { "raw_sfn_hex": "93b9919088d995b7444154" },
                "source": {
                    "kind": "mz_lha_member",
                    "container": "INSTALL.EXE",
                    "member": { "raw_name_hex": "93b9919088d995b72e444154" }
                },
                "source_size": original.len(),
                "source_sha256": sha256_hex(&original),
                "transform": { "kind": "bps" }
            }]
        }
    });
    let plan_json = format!("{}\n", serde_json::to_string_pretty(&plan)?);
    let package = create_patch_package(&plan_json, &source, &content)?;
    let target = apply_patch_package(&source, &package)?;
    let manifest = json!({
        "format": PACKAGE_FORMAT,
        "source_bytes": source.len(),
        "source_sha256": sha256_hex(&source),
        "package_bytes": package.len(),
        "package_sha256": sha256_hex(&package),
        "target_bytes": target.len(),
        "target_sha256": sha256_hex(&target)
    });
    let manifest_json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);

    fs::create_dir_all(&output_directory)
        .with_context(|| format!("create {}", output_directory.display()))?;
    write_new(&source_path, &source)?;
    write_new(&plan_path, plan_json.as_bytes())?;
    write_new(&package_path, &package)?;
    write_new(&target_path, &target)?;
    write_new(&manifest_path, manifest_json.as_bytes())?;
    Ok(())
}

fn make_source(retained: &[u8], fixture: &[u8]) -> Result<Vec<u8>> {
    let installer = mz_with_stored_lha_member(RAW_FIXTURE_LHA_NAME, fixture)?;
    make_ascii_image(&[("KEEP.SYS", retained), ("INSTALL.EXE", &installer)])
}

fn make_content(retained: &[u8], fixture: &[u8]) -> Result<Vec<u8>> {
    let mut image = make_ascii_image(&[("KEEP.SYS", retained), ("DOCHO.DAT", fixture)])?;
    replace_root_name(&mut image, ASCII_FIXTURE_SFN, RAW_FIXTURE_SFN)?;
    Ok(image)
}

fn make_ascii_image(files: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut image = vec![0; BYTES_PER_SECTOR * TOTAL_SECTORS];
    let options = FormatVolumeOptions::new()
        .bytes_per_sector(BYTES_PER_SECTOR as u16)
        .bytes_per_cluster(BYTES_PER_SECTOR as u32)
        .total_sectors(TOTAL_SECTORS as u32)
        .fat_type(FatType::Fat12)
        .max_root_dir_entries(16)
        .fats(2)
        .media(0xf0)
        .sectors_per_track(8)
        .heads(2);
    format_volume(Cursor::new(image.as_mut_slice()), options).context("format FAT12 fixture")?;
    {
        let filesystem = FileSystem::new(Cursor::new(image.as_mut_slice()), FsOptions::new())
            .context("mount FAT12 fixture")?;
        let root = filesystem.root_dir();
        for (name, bytes) in files {
            root.create_file(name)
                .with_context(|| format!("create {name}"))?
                .write_all(bytes)
                .with_context(|| format!("write {name}"))?;
        }
        drop(root);
        filesystem.unmount().context("unmount FAT12 fixture")?;
    }
    Ok(image)
}

fn mz_with_stored_lha_member(raw_name: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let executable_size = 512_usize;
    let mut executable = vec![0_u8; executable_size];
    executable[..2].copy_from_slice(b"MZ");
    executable[4..6].copy_from_slice(&1_u16.to_le_bytes());

    let payload_size = u32::try_from(payload.len()).context("fixture payload is too large")?;
    let mut header_body = Vec::new();
    header_body.extend_from_slice(b"-lh0-");
    header_body.extend_from_slice(&payload_size.to_le_bytes());
    header_body.extend_from_slice(&payload_size.to_le_bytes());
    header_body.extend_from_slice(&0_u32.to_le_bytes());
    header_body.push(0x20);
    header_body.push(0);
    header_body.push(u8::try_from(raw_name.len()).context("fixture LHA name is too long")?);
    header_body.extend_from_slice(raw_name);
    header_body.extend_from_slice(&lha_crc16(payload).to_le_bytes());

    executable.push(u8::try_from(header_body.len()).context("fixture LHA header is too long")?);
    executable.push(header_body.iter().copied().fold(0_u8, u8::wrapping_add));
    executable.extend_from_slice(&header_body);
    executable.extend_from_slice(payload);
    executable.push(0);
    Ok(executable)
}

fn lha_crc16(bytes: &[u8]) -> u16 {
    bytes.iter().copied().fold(0_u16, |mut crc, byte| {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xa001
            };
        }
        crc
    })
}

fn replace_root_name(image: &mut [u8], from: &[u8; 11], to: &[u8; 11]) -> Result<()> {
    let bytes_per_sector = usize::from(u16_at(image, 11)?);
    let reserved_sectors = usize::from(u16_at(image, 14)?);
    let fat_count = usize::from(image[16]);
    let sectors_per_fat = usize::from(u16_at(image, 22)?);
    let root_entries = usize::from(u16_at(image, 17)?);
    let root_offset = reserved_sectors
        .checked_add(
            fat_count
                .checked_mul(sectors_per_fat)
                .context("FAT span overflow")?,
        )
        .and_then(|sectors| sectors.checked_mul(bytes_per_sector))
        .context("root offset overflow")?;
    for index in 0..root_entries {
        let offset = root_offset
            .checked_add(index.checked_mul(32).context("root entry overflow")?)
            .context("root entry offset overflow")?;
        let name = image
            .get_mut(offset..offset + 11)
            .context("truncated root directory")?;
        if name == from {
            name.copy_from_slice(to);
            return Ok(());
        }
    }
    anyhow::bail!("fixture root entry was not found")
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .context("truncated FAT12 BPB")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid FAT12 BPB field"))?,
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("flush {}", path.display()))
}
