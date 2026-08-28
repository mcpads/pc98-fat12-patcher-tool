use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, format_volume};
use pc98_fat12_patcher_core::{apply_patch_package, create_patch_package};
use serde_json::json;
use sha2::{Digest, Sha256};

const BYTES_PER_SECTOR: usize = 512;
const TOTAL_SECTORS: usize = 64;
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let output_directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: cargo run --example write_conformance_fixture -- <output-directory>")?;
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
    let old_first = vec![0xa5; 700];
    let old_second = vec![0xb6; 32];
    let source = make_source(retained, &old_first, &old_second)?;
    let content = make_content(&source)?;
    let plan = json!({
        "id": "canonical-fat12-conformance",
        "title": "Canonical FAT12 conformance fixture",
        "output_filename": "conformance-target.hdm",
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
            "placed_files": [
                {
                    "name": "ZNEW.BIN",
                    "source": { "kind": "root_file", "name": "OLD1.BIN" },
                    "source_size": old_first.len(),
                    "source_sha256": sha256_hex(&old_first),
                    "transform": { "kind": "bps" }
                },
                {
                    "name": "ANEW.BIN",
                    "source": { "kind": "empty" },
                    "source_size": 0,
                    "source_sha256": EMPTY_SHA256,
                    "transform": { "kind": "bps" }
                }
            ]
        }
    });
    let plan_json = format!("{}\n", serde_json::to_string_pretty(&plan)?);
    let package = create_patch_package(&plan_json, &source, &content)?;
    let target = apply_patch_package(&source, &package)?;
    let manifest = json!({
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

fn make_source(retained: &[u8], old_first: &[u8], old_second: &[u8]) -> Result<Vec<u8>> {
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
    format_volume(Cursor::new(image.as_mut_slice()), options).context("format source FAT12")?;
    {
        let filesystem = FileSystem::new(Cursor::new(image.as_mut_slice()), FsOptions::new())
            .context("mount source FAT12")?;
        let root = filesystem.root_dir();
        for (name, bytes) in [
            ("KEEP.SYS", retained),
            ("OLD1.BIN", old_first),
            ("OLD2.BIN", old_second),
        ] {
            root.create_file(name)
                .with_context(|| format!("create {name}"))?
                .write_all(bytes)
                .with_context(|| format!("write {name}"))?;
        }
        drop(root);
        filesystem.unmount().context("unmount source FAT12")?;
    }
    Ok(image)
}

fn make_content(source: &[u8]) -> Result<Vec<u8>> {
    let mut image = source.to_vec();
    {
        let filesystem = FileSystem::new(Cursor::new(image.as_mut_slice()), FsOptions::new())
            .context("mount content FAT12")?;
        let root = filesystem.root_dir();
        root.remove("OLD1.BIN").context("remove OLD1.BIN")?;
        root.remove("OLD2.BIN").context("remove OLD2.BIN")?;
        root.create_file("ZNEW.BIN")
            .context("create ZNEW.BIN")?
            .write_all(&[0x31; 600])
            .context("write ZNEW.BIN")?;
        root.create_file("ANEW.BIN")
            .context("create ANEW.BIN")?
            .write_all(&[0x42; 16])
            .context("write ANEW.BIN")?;
        drop(root);
        filesystem.unmount().context("unmount content FAT12")?;
    }
    Ok(image)
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
