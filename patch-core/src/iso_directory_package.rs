use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use anyhow::{Context, Result, ensure};
use retro_patch_utility::bps::{
    BpsLimits, apply_patch, create_patch, inspect_patch, inspect_patch_statistics,
};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::hash::{require_sha256, sha256_hex, validate_sha256};
use crate::iso9660::{
    IsoFile, RAW_MODE1_SECTOR_SIZE, build_single_directory_iso, extract_raw_mode1_directory,
    validate_iso_path, validate_volume_id,
};
use crate::limits::{
    MAX_BPS_ACTIONS, MAX_BPS_BYTES, MAX_BPS_METADATA_BYTES, MAX_PATCH_PACKAGE_BYTES,
    MAX_RECIPE_BYTES, MAX_ZIP_ENTRIES,
};
use crate::patch_package::{
    RECIPE_ENTRY_NAME, collect_entry_names, patch_entry_name, read_entry, require_patch_entries,
    require_single_entry, write_patch_package,
};
use crate::patch_set::PATCH_SET_ENTRY_NAME;

pub const ISO_DIRECTORY_PACKAGE_FORMAT: &str = "retrogame-patcher-iso9660-directory-file-bps";
const ISO_DIRECTORY_FILE_PATCH_FORMAT: &str = "retrogame-patcher-iso9660-directory-file";
const MAX_RAW_CD_BYTES: usize = 1024 * 1024 * 1024;
const MAX_DIRECTORY_FILES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsoDirectoryPatchPlan {
    pub format: String,
    pub id: String,
    pub title: String,
    pub output_filename: String,
    pub source: IsoDirectorySource,
    pub target: PlannedIsoDirectoryTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsoDirectoryPatchRecipe {
    pub format: String,
    pub id: String,
    pub title: String,
    pub output_filename: String,
    pub source: IsoDirectorySource,
    pub files: Vec<IsoDirectoryFile>,
    pub target: IsoDirectoryTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsoDirectorySource {
    pub size: usize,
    pub sha256: String,
    pub volume_id: String,
    pub directory: String,
    pub expected_file_count: usize,
    pub expected_total_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedIsoDirectoryTarget {
    pub volume_id: String,
    pub directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsoDirectoryTarget {
    pub volume_id: String,
    pub directory: String,
    pub size: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsoDirectoryFile {
    pub name: String,
    pub source_size: usize,
    pub source_sha256: String,
    pub transform: IsoDirectoryFileTransform,
}

impl IsoDirectoryFile {
    pub fn target_size(&self) -> usize {
        match self.transform {
            IsoDirectoryFileTransform::Copy => self.source_size,
            IsoDirectoryFileTransform::Bps { target_size, .. } => target_size,
        }
    }

    pub fn target_sha256(&self) -> &str {
        match &self.transform {
            IsoDirectoryFileTransform::Copy => &self.source_sha256,
            IsoDirectoryFileTransform::Bps { target_sha256, .. } => target_sha256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IsoDirectoryFileTransform {
    Copy,
    Bps {
        target_size: usize,
        target_sha256: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoDirectoryPatchPackage {
    pub recipe_json: String,
    pub recipe: IsoDirectoryPatchRecipe,
    pub patches: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IsoDirectoryFilePatchMetadata {
    format: String,
    recipe_id: String,
    filename: String,
    source_size: usize,
    source_sha256: String,
    target_size: usize,
    target_sha256: String,
}

pub fn parse_iso_directory_plan(json: &str) -> Result<IsoDirectoryPatchPlan> {
    require_json_size(json)?;
    let plan: IsoDirectoryPatchPlan =
        serde_json::from_str(json).context("parse ISO directory patch author plan JSON")?;
    plan.validate()?;
    Ok(plan)
}

pub fn parse_iso_directory_recipe(json: &str) -> Result<IsoDirectoryPatchRecipe> {
    require_json_size(json)?;
    let recipe: IsoDirectoryPatchRecipe =
        serde_json::from_str(json).context("parse ISO directory patch recipe JSON")?;
    recipe.validate()?;
    Ok(recipe)
}

pub fn create_iso_directory_patch_package(
    plan_json: &str,
    source: &[u8],
    content_files: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
    let plan = parse_iso_directory_plan(plan_json)?;
    require_source_image(&plan.source, source)?;
    let source_files = extract_and_validate_source(&plan.source, source)?;
    require_matching_content_names(&source_files, content_files)?;

    let mut files = Vec::with_capacity(source_files.len());
    let mut target_files = Vec::with_capacity(source_files.len());
    for source_file in &source_files {
        let target_bytes = content_files
            .get(&source_file.name)
            .expect("content filenames were matched to source filenames");
        let transform = if target_bytes == &source_file.bytes {
            IsoDirectoryFileTransform::Copy
        } else {
            IsoDirectoryFileTransform::Bps {
                target_size: target_bytes.len(),
                target_sha256: sha256_hex(target_bytes),
            }
        };
        files.push(IsoDirectoryFile {
            name: source_file.name.clone(),
            source_size: source_file.bytes.len(),
            source_sha256: sha256_hex(&source_file.bytes),
            transform,
        });
        target_files.push(IsoFile {
            name: source_file.name.clone(),
            bytes: target_bytes.clone(),
        });
    }

    let target = build_single_directory_iso(
        &plan.target.volume_id,
        &plan.target.directory,
        &target_files,
    )
    .context("build canonical ISO directory target")?;
    let recipe = IsoDirectoryPatchRecipe {
        format: ISO_DIRECTORY_PACKAGE_FORMAT.to_owned(),
        id: plan.id,
        title: plan.title,
        output_filename: plan.output_filename,
        source: plan.source,
        files,
        target: IsoDirectoryTarget {
            volume_id: plan.target.volume_id,
            directory: plan.target.directory,
            size: target.len(),
            sha256: sha256_hex(&target),
        },
    };
    recipe.validate()?;

    let source_by_name = source_files
        .into_iter()
        .map(|file| (file.name, file.bytes))
        .collect::<BTreeMap<_, _>>();
    let target_by_name = target_files
        .into_iter()
        .map(|file| (file.name, file.bytes))
        .collect::<BTreeMap<_, _>>();
    let mut patches = BTreeMap::new();
    for file in &recipe.files {
        if !matches!(file.transform, IsoDirectoryFileTransform::Bps { .. }) {
            continue;
        }
        let patch = create_iso_file_patch(
            &recipe,
            file,
            &source_by_name[&file.name],
            &target_by_name[&file.name],
        )
        .with_context(|| format!("create {} BPS", file.name))?;
        patches.insert(file.name.clone(), patch);
    }

    let recipe_json = format!("{}\n", serde_json::to_string_pretty(&recipe)?);
    let package = write_patch_package(recipe_json.as_bytes(), &patches)?;
    let inspected =
        inspect_iso_directory_patch_package(&package).context("verify newly created patch ZIP")?;
    ensure!(
        inspected.recipe == recipe,
        "new patch ZIP changed its generated ISO directory recipe"
    );
    let reapplied = apply_iso_directory_package_contents(&inspected, source)
        .context("self-apply newly created ISO directory patch ZIP")?;
    ensure!(
        reapplied == target,
        "new ISO directory patch ZIP did not reproduce its target byte-for-byte"
    );
    Ok(package)
}

pub fn inspect_iso_directory_patch_package(package: &[u8]) -> Result<IsoDirectoryPatchPackage> {
    ensure!(
        package.len() <= MAX_PATCH_PACKAGE_BYTES,
        "patch ZIP is too large: {} bytes exceeds {MAX_PATCH_PACKAGE_BYTES}",
        package.len()
    );
    let mut archive = ZipArchive::new(Cursor::new(package)).context("open patch ZIP")?;
    ensure!(
        archive.len() <= MAX_ZIP_ENTRIES,
        "patch ZIP has too many entries: {} exceeds {MAX_ZIP_ENTRIES}",
        archive.len()
    );
    let names = collect_entry_names(&mut archive)?;
    require_single_entry(&names, RECIPE_ENTRY_NAME)?;
    ensure!(
        !names.contains_key(PATCH_SET_ENTRY_NAME),
        "patch ZIP cannot contain both {RECIPE_ENTRY_NAME} and {PATCH_SET_ENTRY_NAME}"
    );
    let recipe_bytes = read_entry(
        &mut archive,
        RECIPE_ENTRY_NAME,
        u64::try_from(MAX_RECIPE_BYTES).context("recipe size limit does not fit ZIP limits")?,
    )?;
    let recipe_json = String::from_utf8(recipe_bytes).context("recipe.json is not UTF-8")?;
    let recipe = parse_iso_directory_recipe(&recipe_json)
        .context("parse ISO directory recipe.json from patch ZIP")?;
    let expected_entries = recipe
        .files
        .iter()
        .filter(|file| matches!(file.transform, IsoDirectoryFileTransform::Bps { .. }))
        .map(|file| (file.name.clone(), patch_entry_name(&file.name)))
        .collect::<BTreeMap<_, _>>();
    require_patch_entries(&names, &expected_entries)?;

    let mut extracted_bytes = 0usize;
    let mut patches = BTreeMap::new();
    for file in &recipe.files {
        if !matches!(file.transform, IsoDirectoryFileTransform::Bps { .. }) {
            continue;
        }
        let entry_name = expected_entries
            .get(&file.name)
            .expect("entry names were derived from recipe files");
        let patch = read_entry(&mut archive, entry_name, MAX_BPS_BYTES as u64)?;
        extracted_bytes = extracted_bytes
            .checked_add(patch.len())
            .context("total BPS payload size overflow")?;
        ensure!(
            extracted_bytes <= MAX_BPS_BYTES,
            "BPS payloads expand to {extracted_bytes} bytes, exceeding {MAX_BPS_BYTES}"
        );
        inspect_iso_file_patch(&recipe, file, &patch)
            .with_context(|| format!("validate {entry_name}"))?;
        patches.insert(file.name.clone(), patch);
    }
    Ok(IsoDirectoryPatchPackage {
        recipe_json,
        recipe,
        patches,
    })
}

pub fn apply_iso_directory_patch_package(source: &[u8], package: &[u8]) -> Result<Vec<u8>> {
    let contents = inspect_iso_directory_patch_package(package)?;
    apply_iso_directory_package_contents(&contents, source)
}

fn apply_iso_directory_package_contents(
    contents: &IsoDirectoryPatchPackage,
    source: &[u8],
) -> Result<Vec<u8>> {
    let recipe = &contents.recipe;
    require_source_image(&recipe.source, source)?;
    let source_files = extract_and_validate_source(&recipe.source, source)?;
    let source_by_name = source_files
        .into_iter()
        .map(|file| (file.name, file.bytes))
        .collect::<BTreeMap<_, _>>();
    let mut target_files = Vec::with_capacity(recipe.files.len());
    for file in &recipe.files {
        let source_bytes = source_by_name
            .get(&file.name)
            .with_context(|| format!("source directory is missing {}", file.name))?;
        require_file_source(file, source_bytes)?;
        let bytes = match file.transform {
            IsoDirectoryFileTransform::Copy => source_bytes.clone(),
            IsoDirectoryFileTransform::Bps { .. } => {
                let patch = contents
                    .patches
                    .get(&file.name)
                    .with_context(|| format!("patch package is missing {} BPS", file.name))?;
                apply_iso_file_patch(recipe, file, source_bytes, patch)?
            }
        };
        target_files.push(IsoFile {
            name: file.name.clone(),
            bytes,
        });
    }
    let target = build_single_directory_iso(
        &recipe.target.volume_id,
        &recipe.target.directory,
        &target_files,
    )
    .context("assemble patched ISO")?;
    ensure!(
        target.len() == recipe.target.size,
        "target ISO size mismatch: expected {}, got {}",
        recipe.target.size,
        target.len()
    );
    require_sha256(&target, &recipe.target.sha256, "target ISO")?;
    Ok(target)
}

fn create_iso_file_patch(
    recipe: &IsoDirectoryPatchRecipe,
    file: &IsoDirectoryFile,
    source: &[u8],
    target: &[u8],
) -> Result<Vec<u8>> {
    require_file_source(file, source)?;
    require_file_target(file, target)?;
    let metadata = encode_file_metadata(recipe, file)?;
    let patch = create_patch(source, target, &metadata).context("create ISO file BPS")?;
    inspect_iso_file_patch(recipe, file, &patch)?;
    let reapplied = apply_iso_file_patch(recipe, file, source, &patch)?;
    ensure!(
        reapplied == target,
        "new BPS did not reproduce {}",
        file.name
    );
    Ok(patch)
}

fn inspect_iso_file_patch(
    recipe: &IsoDirectoryPatchRecipe,
    file: &IsoDirectoryFile,
    patch: &[u8],
) -> Result<()> {
    let target_size = require_bps_transform(file)?.0;
    let limits = bps_limits(file.source_size, target_size);
    let info = inspect_patch(patch, limits).context("inspect ISO file BPS header")?;
    ensure!(
        info.source_size == file.source_size,
        "{} BPS source size differs from recipe",
        file.name
    );
    ensure!(
        info.target_size == target_size,
        "{} BPS target size differs from recipe",
        file.name
    );
    ensure!(
        info.metadata == encode_file_metadata(recipe, file)?,
        "{} BPS metadata does not match its recipe binding",
        file.name
    );
    inspect_patch_statistics(patch, limits).context("validate ISO file BPS action stream")?;
    Ok(())
}

fn apply_iso_file_patch(
    recipe: &IsoDirectoryPatchRecipe,
    file: &IsoDirectoryFile,
    source: &[u8],
    patch: &[u8],
) -> Result<Vec<u8>> {
    require_file_source(file, source)?;
    let target_size = require_bps_transform(file)?.0;
    inspect_iso_file_patch(recipe, file, patch)?;
    let applied = apply_patch(source, patch, bps_limits(file.source_size, target_size))
        .context("apply ISO file BPS")?;
    require_file_target(file, &applied.target)?;
    Ok(applied.target)
}

fn encode_file_metadata(
    recipe: &IsoDirectoryPatchRecipe,
    file: &IsoDirectoryFile,
) -> Result<Vec<u8>> {
    let (target_size, target_sha256) = require_bps_transform(file)?;
    serde_json::to_vec(&IsoDirectoryFilePatchMetadata {
        format: ISO_DIRECTORY_FILE_PATCH_FORMAT.to_owned(),
        recipe_id: recipe.id.clone(),
        filename: file.name.clone(),
        source_size: file.source_size,
        source_sha256: file.source_sha256.clone(),
        target_size,
        target_sha256: target_sha256.to_owned(),
    })
    .context("serialize ISO file BPS metadata")
}

fn bps_limits(source_size: usize, target_size: usize) -> BpsLimits {
    BpsLimits::new(
        MAX_BPS_BYTES,
        source_size,
        target_size,
        MAX_BPS_METADATA_BYTES,
        MAX_BPS_ACTIONS,
    )
}

fn require_bps_transform(file: &IsoDirectoryFile) -> Result<(usize, &str)> {
    match &file.transform {
        IsoDirectoryFileTransform::Bps {
            target_size,
            target_sha256,
        } => Ok((*target_size, target_sha256)),
        IsoDirectoryFileTransform::Copy => {
            anyhow::bail!("{} does not declare a BPS transform", file.name)
        }
    }
}

fn require_file_source(file: &IsoDirectoryFile, source: &[u8]) -> Result<()> {
    ensure!(
        source.len() == file.source_size,
        "{} source size mismatch: expected {}, got {}",
        file.name,
        file.source_size,
        source.len()
    );
    require_sha256(
        source,
        &file.source_sha256,
        &format!("{} source", file.name),
    )
}

fn require_file_target(file: &IsoDirectoryFile, target: &[u8]) -> Result<()> {
    ensure!(
        target.len() == file.target_size(),
        "{} target size mismatch: expected {}, got {}",
        file.name,
        file.target_size(),
        target.len()
    );
    require_sha256(
        target,
        file.target_sha256(),
        &format!("{} target", file.name),
    )
}

fn require_source_image(profile: &IsoDirectorySource, source: &[u8]) -> Result<()> {
    ensure!(
        source.len() == profile.size,
        "source CD image size mismatch: expected {}, got {}",
        profile.size,
        source.len()
    );
    require_sha256(source, &profile.sha256, "source CD image")
}

fn extract_and_validate_source(
    profile: &IsoDirectorySource,
    source: &[u8],
) -> Result<Vec<IsoFile>> {
    let files = extract_raw_mode1_directory(source, &profile.volume_id, &profile.directory)
        .context("extract declared source ISO directory")?;
    ensure!(
        files.len() == profile.expected_file_count,
        "source ISO directory file count mismatch: expected {}, got {}",
        profile.expected_file_count,
        files.len()
    );
    let total_size = total_file_size(files.iter().map(|file| file.bytes.len()))?;
    ensure!(
        total_size == profile.expected_total_size,
        "source ISO directory byte count mismatch: expected {}, got {total_size}",
        profile.expected_total_size
    );
    Ok(files)
}

fn require_matching_content_names(
    source_files: &[IsoFile],
    content_files: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let source_names = source_files
        .iter()
        .map(|file| file.name.as_str())
        .collect::<BTreeSet<_>>();
    let content_names = content_files
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        source_names == content_names,
        "content directory filenames differ from source ISO directory: missing {:?}, extra {:?}",
        source_names.difference(&content_names).collect::<Vec<_>>(),
        content_names.difference(&source_names).collect::<Vec<_>>()
    );
    Ok(())
}

impl IsoDirectoryPatchPlan {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format == ISO_DIRECTORY_PACKAGE_FORMAT,
            "unsupported ISO directory patch plan format: {:?}",
            self.format
        );
        validate_identity(&self.id, &self.title, &self.output_filename)?;
        self.source.validate()?;
        self.target.validate()
    }
}

impl IsoDirectoryPatchRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format == ISO_DIRECTORY_PACKAGE_FORMAT,
            "unsupported ISO directory patch recipe format: {:?}",
            self.format
        );
        validate_identity(&self.id, &self.title, &self.output_filename)?;
        self.source.validate()?;
        self.target.validate()?;
        ensure!(
            self.files.len() == self.source.expected_file_count,
            "recipe file count differs from source profile"
        );
        let mut names = BTreeSet::new();
        let mut previous_name: Option<&str> = None;
        for file in &self.files {
            crate::iso9660::validate_file_identifier(&file.name)?;
            ensure!(
                previous_name.is_none_or(|previous| previous < file.name.as_str()),
                "recipe files must be strictly sorted by filename"
            );
            previous_name = Some(&file.name);
            ensure!(
                names.insert(&file.name),
                "duplicate recipe filename {}",
                file.name
            );
            ensure!(
                file.source_size <= self.source.expected_total_size,
                "{} source size exceeds declared source directory bytes",
                file.name
            );
            validate_sha256(&file.source_sha256, &format!("{} source", file.name))?;
            if let IsoDirectoryFileTransform::Bps {
                target_size,
                target_sha256,
            } = &file.transform
            {
                ensure!(
                    *target_size <= MAX_BPS_BYTES,
                    "{} target is too large",
                    file.name
                );
                validate_sha256(target_sha256, &format!("{} target", file.name))?;
            }
        }
        let source_total = total_file_size(self.files.iter().map(|file| file.source_size))?;
        ensure!(
            source_total == self.source.expected_total_size,
            "recipe source file bytes differ from source profile"
        );
        Ok(())
    }
}

impl IsoDirectorySource {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.size > 0 && self.size <= MAX_RAW_CD_BYTES,
            "source raw CD size must be between 1 and {MAX_RAW_CD_BYTES} bytes"
        );
        ensure!(
            self.size.is_multiple_of(RAW_MODE1_SECTOR_SIZE),
            "source raw CD size must be a multiple of {RAW_MODE1_SECTOR_SIZE}"
        );
        validate_sha256(&self.sha256, "source CD image")?;
        ensure!(
            !self.volume_id.trim().is_empty()
                && self.volume_id.len() <= 32
                && self
                    .volume_id
                    .bytes()
                    .all(|byte| (0x20..=0x7e).contains(&byte)),
            "source ISO volume ID must contain 1 to 32 printable ASCII bytes"
        );
        validate_iso_path(&self.directory)?;
        ensure!(
            (1..=MAX_DIRECTORY_FILES).contains(&self.expected_file_count),
            "source ISO directory file count must be 1..={MAX_DIRECTORY_FILES}"
        );
        ensure!(
            self.expected_total_size <= self.size,
            "source ISO directory bytes exceed source image size"
        );
        Ok(())
    }
}

impl PlannedIsoDirectoryTarget {
    fn validate(&self) -> Result<()> {
        validate_volume_id(&self.volume_id)?;
        validate_iso_path(&self.directory)?;
        ensure!(
            !self.directory.contains('/'),
            "target ISO must contain one root directory"
        );
        Ok(())
    }
}

impl IsoDirectoryTarget {
    fn validate(&self) -> Result<()> {
        PlannedIsoDirectoryTarget {
            volume_id: self.volume_id.clone(),
            directory: self.directory.clone(),
        }
        .validate()?;
        ensure!(self.size > 0, "target ISO size must be positive");
        validate_sha256(&self.sha256, "target ISO")
    }
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

fn total_file_size(mut sizes: impl Iterator<Item = usize>) -> Result<usize> {
    sizes
        .try_fold(0usize, |total, size| total.checked_add(size))
        .context("ISO directory total file size overflow")
}

fn require_json_size(json: &str) -> Result<()> {
    ensure!(
        json.len() <= MAX_RECIPE_BYTES,
        "ISO directory patch JSON is too large: {} bytes exceeds {MAX_RECIPE_BYTES}",
        json.len()
    );
    Ok(())
}

pub(crate) fn recipe_format_from_json(json: &str) -> Result<Option<String>> {
    require_json_size(json)?;
    let value: serde_json::Value =
        serde_json::from_str(json).context("parse recipe JSON marker")?;
    Ok(value
        .as_object()
        .and_then(|object| object.get("format"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}

#[cfg(test)]
#[path = "iso_directory_package_tests.rs"]
mod tests;
