use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::hash::validate_sha256;
use crate::limits::{MAX_HDM_BYTES, MAX_RECIPE_BYTES};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchRecipe {
    pub id: String,
    pub title: String,
    pub output_filename: String,
    pub source: SourceImage,
    pub assembly: AssemblyRecipe,
    pub target: TargetImage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceImage {
    pub size: usize,
    pub sha256: String,
    pub geometry: Fat12Geometry,
    pub mount_policy: MountPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fat12Geometry {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub root_entries: u16,
    pub total_sectors: u16,
    pub media_descriptor: u8,
    pub sectors_per_fat: u16,
    pub sectors_per_track: u16,
    pub heads: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountPolicy {
    Standard,
    Pc98Dos3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyRecipe {
    pub baseline_sha256: String,
    pub retained_files: Vec<ExactFile>,
    pub placed_files: Vec<PlacedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactFile {
    pub name: String,
    pub size: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedFile {
    pub name: String,
    pub source: FileSource,
    pub size: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileSource {
    RootFile { name: String },
    MzLhaMember { container: String, member: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetImage {
    pub size: usize,
    pub sha256: String,
}

pub fn parse_recipe(json: &str) -> Result<PatchRecipe> {
    ensure!(
        json.len() <= MAX_RECIPE_BYTES,
        "patch recipe is too large: {} bytes exceeds {MAX_RECIPE_BYTES}",
        json.len()
    );
    let recipe: PatchRecipe = serde_json::from_str(json).context("parse patch recipe JSON")?;
    recipe.validate()?;
    Ok(recipe)
}

impl PatchRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.id.trim().is_empty(), "recipe id cannot be empty");
        ensure!(
            !self.title.trim().is_empty(),
            "recipe title cannot be empty"
        );
        ensure!(
            !self.output_filename.trim().is_empty(),
            "output filename cannot be empty"
        );
        ensure!(
            !self.output_filename.contains(['/', '\\']),
            "output filename must not contain a path"
        );
        ensure!(self.source.size > 0, "source image size must be positive");
        ensure!(
            self.source.size <= MAX_HDM_BYTES,
            "source HDM is too large: {} bytes exceeds {MAX_HDM_BYTES}",
            self.source.size
        );
        ensure!(
            self.target.size == self.source.size,
            "target HDM size must equal source HDM size"
        );
        validate_sha256(&self.source.sha256, "source image")?;
        validate_sha256(&self.assembly.baseline_sha256, "baseline image")?;
        validate_sha256(&self.target.sha256, "target image")?;
        self.source.geometry.validate(self.source.size)?;

        let output_file_count = self
            .assembly
            .retained_files
            .len()
            .checked_add(self.assembly.placed_files.len())
            .context("assembly file count overflow")?;
        ensure!(
            output_file_count <= usize::from(self.source.geometry.root_entries),
            "assembly declares {output_file_count} root files but geometry has {} root entries",
            self.source.geometry.root_entries
        );
        let mut output_names = BTreeSet::new();
        for file in &self.assembly.retained_files {
            validate_exact_file(file, "retained file", self.source.size)?;
            ensure!(
                output_names.insert(file.name.as_str()),
                "duplicate output file name: {}",
                file.name
            );
        }
        for file in &self.assembly.placed_files {
            validate_dos_name(&file.name, "placed file name")?;
            validate_sha256(&file.sha256, &format!("placed file {}", file.name))?;
            ensure!(
                file.size <= self.source.size,
                "placed file {} is larger than the source HDM",
                file.name
            );
            ensure!(
                output_names.insert(file.name.as_str()),
                "duplicate output file name: {}",
                file.name
            );
            match &file.source {
                FileSource::RootFile { name } => validate_dos_name(name, "root source file")?,
                FileSource::MzLhaMember { container, member } => {
                    validate_dos_name(container, "LHA container")?;
                    validate_dos_name(member, "LHA member")?;
                }
            }
        }
        ensure!(
            !output_names.is_empty(),
            "assembly file set cannot be empty"
        );
        let output_file_bytes = self
            .assembly
            .retained_files
            .iter()
            .map(|file| file.size)
            .chain(self.assembly.placed_files.iter().map(|file| file.size))
            .try_fold(0usize, |total, size| total.checked_add(size))
            .context("assembly output file size overflow")?;
        ensure!(
            output_file_bytes <= self.source.size,
            "assembly declares {output_file_bytes} bytes of root files but the source HDM is {} bytes",
            self.source.size
        );
        Ok(())
    }
}

impl Fat12Geometry {
    fn validate(&self, image_size: usize) -> Result<()> {
        ensure!(
            self.bytes_per_sector.is_power_of_two() && self.bytes_per_sector >= 512,
            "FAT12 bytes per sector must be a power of two at least 512"
        );
        ensure!(
            self.sectors_per_cluster.is_power_of_two() && self.sectors_per_cluster > 0,
            "FAT12 sectors per cluster must be a positive power of two"
        );
        ensure!(
            self.reserved_sectors > 0,
            "FAT12 reserved sectors cannot be zero"
        );
        ensure!(
            (1..=2).contains(&self.fat_count),
            "FAT count must be one or two"
        );
        ensure!(
            self.root_entries > 0,
            "FAT12 root entry count cannot be zero"
        );
        ensure!(self.total_sectors > 0, "FAT12 total sectors cannot be zero");
        ensure!(
            self.sectors_per_fat > 0,
            "FAT12 sectors per FAT cannot be zero"
        );
        ensure!(
            self.sectors_per_track > 0,
            "sectors per track cannot be zero"
        );
        ensure!(self.heads > 0, "head count cannot be zero");
        let declared_size = usize::from(self.bytes_per_sector)
            .checked_mul(usize::from(self.total_sectors))
            .context("FAT12 declared size overflow")?;
        ensure!(
            declared_size == image_size,
            "FAT12 geometry declares {declared_size} bytes, source profile declares {image_size}"
        );
        let root_sectors =
            (u32::from(self.root_entries) * 32).div_ceil(u32::from(self.bytes_per_sector));
        let data_start = u32::from(self.reserved_sectors)
            + u32::from(self.fat_count) * u32::from(self.sectors_per_fat)
            + root_sectors;
        ensure!(
            data_start < u32::from(self.total_sectors),
            "FAT12 metadata consumes the whole image"
        );
        let clusters =
            (u32::from(self.total_sectors) - data_start) / u32::from(self.sectors_per_cluster);
        ensure!(
            clusters < 4085,
            "geometry has {clusters} clusters and is not FAT12"
        );
        Ok(())
    }
}

fn validate_exact_file(file: &ExactFile, label: &str, image_size: usize) -> Result<()> {
    validate_dos_name(&file.name, label)?;
    validate_sha256(&file.sha256, &format!("{label} {}", file.name))?;
    ensure!(
        file.size <= image_size,
        "{label} {} is larger than the source HDM",
        file.name
    );
    Ok(())
}

fn validate_dos_name(name: &str, label: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name == name.to_ascii_uppercase(),
        "{label} must be an uppercase DOS 8.3 name: {name:?}"
    );
    ensure!(
        name.bytes()
            .all(|byte| byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'$' | b'~' | b'-' | b'.')),
        "{label} contains unsupported characters: {name:?}"
    );
    let mut parts = name.split('.');
    let stem = parts.next().unwrap_or_default();
    let extension = parts.next();
    ensure!(
        parts.next().is_none(),
        "{label} is not a DOS 8.3 name: {name:?}"
    );
    ensure!(
        (1..=8).contains(&stem.len()),
        "{label} stem is not 1..=8 bytes: {name:?}"
    );
    if let Some(extension) = extension {
        ensure!(
            (1..=3).contains(&extension.len()),
            "{label} extension is not 1..=3 bytes: {name:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "recipe_tests.rs"]
mod recipe_tests;
