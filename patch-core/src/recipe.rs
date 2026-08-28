use std::collections::BTreeSet;
use std::fmt;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::hash::{EMPTY_SHA256, validate_sha256};
use crate::limits::{MAX_HDM_BYTES, MAX_RECIPE_BYTES};

pub const PACKAGE_FORMAT: &str = "retrogame-patcher-pc98-fat12-file-bps";

#[derive(Debug)]
pub(crate) struct UnsupportedPackageFormat;

impl fmt::Display for UnsupportedPackageFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsupported patch package format; expected {PACKAGE_FORMAT}"
        )
    }
}

impl std::error::Error for UnsupportedPackageFormat {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchPlan {
    pub id: String,
    pub title: String,
    pub output_filename: String,
    pub source: SourceImage,
    pub assembly: PlannedAssemblyRecipe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchRecipe {
    pub format: String,
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
pub struct PlannedAssemblyRecipe {
    pub retained_files: Vec<ExactFile>,
    pub placed_files: Vec<PlannedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyRecipe {
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
pub struct PlannedFile {
    pub name: String,
    pub source: FileSource,
    pub source_size: usize,
    pub source_sha256: String,
    pub transform: PlannedTransform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlannedTransform {
    Copy,
    Bps,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedFile {
    pub name: String,
    pub source: FileSource,
    pub source_size: usize,
    pub source_sha256: String,
    pub transform: FileTransform,
}

impl PlacedFile {
    pub fn target_size(&self) -> usize {
        match self.transform {
            FileTransform::Copy => self.source_size,
            FileTransform::Bps { target_size, .. } => target_size,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileTransform {
    Copy,
    Bps {
        target_size: usize,
        target_sha256: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileSource {
    RootFile { name: String },
    MzLhaMember { container: String, member: String },
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetImage {
    pub size: usize,
    pub sha256: String,
}

pub fn parse_plan(json: &str) -> Result<PatchPlan> {
    require_recipe_size(json)?;
    let plan: PatchPlan = serde_json::from_str(json).context("parse patch author plan JSON")?;
    plan.validate()?;
    Ok(plan)
}

pub fn parse_recipe(json: &str) -> Result<PatchRecipe> {
    require_recipe_size(json)?;
    let value: serde_json::Value = serde_json::from_str(json).context("parse patch recipe JSON")?;
    let format = value
        .as_object()
        .and_then(|object| object.get("format"))
        .and_then(serde_json::Value::as_str);
    if format != Some(PACKAGE_FORMAT) {
        return Err(UnsupportedPackageFormat.into());
    }
    let recipe: PatchRecipe = serde_json::from_value(value).context("parse patch recipe JSON")?;
    recipe.validate()?;
    Ok(recipe)
}

impl PatchPlan {
    pub fn validate(&self) -> Result<()> {
        validate_identity(&self.id, &self.title, &self.output_filename)?;
        validate_source_image(&self.source)?;
        validate_output_count(
            &self.source,
            self.assembly.retained_files.len(),
            self.assembly.placed_files.len(),
        )?;
        let mut output_names =
            validate_retained_files(&self.assembly.retained_files, self.source.size)?;
        for file in &self.assembly.placed_files {
            validate_placed_source(
                &file.name,
                &file.source,
                file.source_size,
                &file.source_sha256,
                self.source.size,
            )?;
            ensure!(
                output_names.insert(file.name.as_str()),
                "duplicate output file name: {}",
                file.name
            );
        }
        require_nonempty_assembly(&output_names)?;
        let declared_bytes = self
            .assembly
            .retained_files
            .iter()
            .map(|file| file.size)
            .chain(
                self.assembly
                    .placed_files
                    .iter()
                    .map(|file| file.source_size),
            );
        validate_declared_file_bytes(declared_bytes, self.source.size)
    }
}

impl PatchRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format == PACKAGE_FORMAT,
            "recipe format must be {PACKAGE_FORMAT}"
        );
        validate_identity(&self.id, &self.title, &self.output_filename)?;
        validate_source_image(&self.source)?;
        ensure!(
            self.target.size == self.source.size,
            "target HDM size must equal source HDM size"
        );
        validate_sha256(&self.target.sha256, "target image")?;
        validate_output_count(
            &self.source,
            self.assembly.retained_files.len(),
            self.assembly.placed_files.len(),
        )?;
        let mut output_names =
            validate_retained_files(&self.assembly.retained_files, self.source.size)?;
        for file in &self.assembly.placed_files {
            validate_placed_source(
                &file.name,
                &file.source,
                file.source_size,
                &file.source_sha256,
                self.source.size,
            )?;
            match &file.transform {
                FileTransform::Copy => {}
                FileTransform::Bps {
                    target_size,
                    target_sha256,
                } => {
                    ensure!(
                        *target_size <= self.source.size,
                        "target file {} is larger than the source HDM",
                        file.name
                    );
                    validate_sha256(target_sha256, &format!("target file {}", file.name))?;
                }
            }
            ensure!(
                output_names.insert(file.name.as_str()),
                "duplicate output file name: {}",
                file.name
            );
        }
        require_nonempty_assembly(&output_names)?;
        let declared_bytes = self
            .assembly
            .retained_files
            .iter()
            .map(|file| file.size)
            .chain(
                self.assembly
                    .placed_files
                    .iter()
                    .map(PlacedFile::target_size),
            );
        validate_declared_file_bytes(declared_bytes, self.source.size)
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

fn require_recipe_size(json: &str) -> Result<()> {
    ensure!(
        json.len() <= MAX_RECIPE_BYTES,
        "patch recipe is too large: {} bytes exceeds {MAX_RECIPE_BYTES}",
        json.len()
    );
    Ok(())
}

fn validate_identity(id: &str, title: &str, output_filename: &str) -> Result<()> {
    ensure!(!id.trim().is_empty(), "recipe id cannot be empty");
    ensure!(!title.trim().is_empty(), "recipe title cannot be empty");
    ensure!(
        !output_filename.trim().is_empty(),
        "output filename cannot be empty"
    );
    ensure!(
        !output_filename.contains(['/', '\\']),
        "output filename must not contain a path"
    );
    Ok(())
}

fn validate_source_image(source: &SourceImage) -> Result<()> {
    ensure!(source.size > 0, "source image size must be positive");
    ensure!(
        source.size <= MAX_HDM_BYTES,
        "source HDM is too large: {} bytes exceeds {MAX_HDM_BYTES}",
        source.size
    );
    validate_sha256(&source.sha256, "source image")?;
    source.geometry.validate(source.size)
}

fn validate_output_count(source: &SourceImage, retained: usize, placed: usize) -> Result<()> {
    let output_file_count = retained
        .checked_add(placed)
        .context("assembly file count overflow")?;
    ensure!(
        output_file_count <= usize::from(source.geometry.root_entries),
        "assembly declares {output_file_count} root files but geometry has {} root entries",
        source.geometry.root_entries
    );
    Ok(())
}

fn validate_retained_files(
    retained_files: &[ExactFile],
    image_size: usize,
) -> Result<BTreeSet<&str>> {
    let mut names = BTreeSet::new();
    for file in retained_files {
        validate_dos_name(&file.name, "retained file")?;
        validate_sha256(&file.sha256, &format!("retained file {}", file.name))?;
        ensure!(
            file.size <= image_size,
            "retained file {} is larger than the source HDM",
            file.name
        );
        ensure!(
            names.insert(file.name.as_str()),
            "duplicate output file name: {}",
            file.name
        );
    }
    Ok(names)
}

fn validate_placed_source(
    output_name: &str,
    source: &FileSource,
    source_size: usize,
    source_sha256: &str,
    image_size: usize,
) -> Result<()> {
    validate_dos_name(output_name, "placed file name")?;
    validate_sha256(source_sha256, &format!("source file {output_name}"))?;
    ensure!(
        source_size <= image_size,
        "source file {output_name} is larger than the source HDM"
    );
    match source {
        FileSource::RootFile { name } => validate_dos_name(name, "root source file"),
        FileSource::MzLhaMember { container, member } => {
            validate_dos_name(container, "LHA container")?;
            validate_dos_name(member, "LHA member")
        }
        FileSource::Empty => {
            ensure!(
                source_size == 0,
                "empty source for {output_name} must have size 0"
            );
            ensure!(
                source_sha256 == EMPTY_SHA256,
                "empty source for {output_name} must have the SHA-256 of zero bytes"
            );
            Ok(())
        }
    }
}

fn validate_declared_file_bytes(
    mut sizes: impl Iterator<Item = usize>,
    image_size: usize,
) -> Result<()> {
    let output_file_bytes = sizes
        .try_fold(0usize, |total, size| total.checked_add(size))
        .context("assembly output file size overflow")?;
    ensure!(
        output_file_bytes <= image_size,
        "assembly declares {output_file_bytes} bytes of root files but the source HDM is {image_size} bytes"
    );
    Ok(())
}

fn require_nonempty_assembly(names: &BTreeSet<&str>) -> Result<()> {
    ensure!(!names.is_empty(), "assembly file set cannot be empty");
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
