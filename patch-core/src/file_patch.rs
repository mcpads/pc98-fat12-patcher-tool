use anyhow::{Context, Result, ensure};
use retro_patch_utility::bps::{
    BpsLimits, PatchStatistics, apply_patch, create_patch, inspect_patch, inspect_patch_statistics,
};
use serde::{Deserialize, Serialize};

use crate::hash::require_sha256;
use crate::limits::{MAX_BPS_ACTIONS, MAX_BPS_BYTES, MAX_BPS_METADATA_BYTES};
use crate::recipe::{FileTransform, PackageFormat, PatchRecipe, PlacedFile};

const LEGACY_FILE_PATCH_FORMAT: &str = "retrogame-patcher-pc98-fat12-file";
const FILE_PATCH_FORMAT: &str = "retrogame-patcher-pc98-fat12-raw-sfn-file";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePatchMetadata {
    format: String,
    recipe_id: String,
    output_name: String,
    source_size: usize,
    source_sha256: String,
    target_size: usize,
    target_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSfnFilePatchMetadata {
    format: String,
    recipe_id: String,
    patch_key: String,
    target_sfn_hex: String,
    source_size: usize,
    source_sha256: String,
    target_size: usize,
    target_sha256: String,
}

pub(crate) fn create_file_patch(
    recipe: &PatchRecipe,
    file: &PlacedFile,
    source: &[u8],
    target: &[u8],
) -> Result<Vec<u8>> {
    require_source(file, source)?;
    require_target(file, target)?;
    let metadata = encode_metadata(recipe, file)?;
    let patch = create_patch(source, target, &metadata).context("create file BPS")?;
    inspect_file_patch(recipe, file, &patch).context("validate newly created file BPS")?;
    let reapplied = apply_file_patch(recipe, file, source, &patch)?;
    ensure!(
        reapplied == target,
        "new BPS did not reproduce {} byte-for-byte",
        file.name
    );
    Ok(patch)
}

pub(crate) fn inspect_file_patch(
    recipe: &PatchRecipe,
    file: &PlacedFile,
    patch: &[u8],
) -> Result<PatchStatistics> {
    let (target_size, _) = bps_target(file)?;
    let limits = limits_for(file.source_size, target_size);
    let info = inspect_patch(patch, limits).context("inspect file BPS header")?;
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
    validate_metadata(recipe, file, &info.metadata)?;
    inspect_patch_statistics(patch, limits).context("validate file BPS action stream")
}

pub(crate) fn apply_file_patch(
    recipe: &PatchRecipe,
    file: &PlacedFile,
    source: &[u8],
    patch: &[u8],
) -> Result<Vec<u8>> {
    require_source(file, source)?;
    let (target_size, _) = bps_target(file)?;
    inspect_file_patch(recipe, file, patch)?;
    let applied = apply_patch(source, patch, limits_for(file.source_size, target_size))
        .context("apply file BPS")?;
    require_target(file, &applied.target)?;
    Ok(applied.target)
}

fn limits_for(source_size: usize, target_size: usize) -> BpsLimits {
    BpsLimits::new(
        MAX_BPS_BYTES,
        source_size,
        target_size,
        MAX_BPS_METADATA_BYTES,
        MAX_BPS_ACTIONS,
    )
}

fn require_source(file: &PlacedFile, source: &[u8]) -> Result<()> {
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

fn require_target(file: &PlacedFile, target: &[u8]) -> Result<()> {
    let (target_size, target_sha256) = bps_target(file)?;
    ensure!(
        target.len() == target_size,
        "{} target size mismatch: expected {target_size}, got {}",
        file.name,
        target.len()
    );
    require_sha256(target, target_sha256, &format!("{} target", file.name))
}

fn bps_target(file: &PlacedFile) -> Result<(usize, &str)> {
    match &file.transform {
        FileTransform::Bps {
            target_size,
            target_sha256,
        } => Ok((*target_size, target_sha256)),
        FileTransform::Copy => anyhow::bail!("{} does not declare a BPS transform", file.name),
    }
}

fn encode_metadata(recipe: &PatchRecipe, file: &PlacedFile) -> Result<Vec<u8>> {
    let (target_size, target_sha256) = bps_target(file)?;
    match recipe.package_format()? {
        PackageFormat::LegacyAscii => {
            let metadata = FilePatchMetadata {
                format: LEGACY_FILE_PATCH_FORMAT.to_owned(),
                recipe_id: recipe.id.clone(),
                output_name: file
                    .name
                    .ascii_name()
                    .context("legacy file patch requires an ASCII output name")?
                    .to_owned(),
                source_size: file.source_size,
                source_sha256: file.source_sha256.clone(),
                target_size,
                target_sha256: target_sha256.to_owned(),
            };
            serde_json::to_vec(&metadata).context("serialize legacy file BPS metadata")
        }
        PackageFormat::RawShortName => {
            let metadata = RawSfnFilePatchMetadata {
                format: FILE_PATCH_FORMAT.to_owned(),
                recipe_id: recipe.id.clone(),
                patch_key: file
                    .effective_patch_key(PackageFormat::RawShortName)?
                    .to_owned(),
                target_sfn_hex: file.name.raw_hex("placed file name")?,
                source_size: file.source_size,
                source_sha256: file.source_sha256.clone(),
                target_size,
                target_sha256: target_sha256.to_owned(),
            };
            serde_json::to_vec(&metadata).context("serialize raw-SFN file BPS metadata")
        }
    }
}

fn validate_metadata(recipe: &PatchRecipe, file: &PlacedFile, metadata: &[u8]) -> Result<()> {
    let expected = encode_metadata(recipe, file)?;
    ensure!(
        metadata == expected,
        "{} BPS metadata does not match its recipe binding",
        file.name
    );
    Ok(())
}

#[cfg(test)]
#[path = "file_patch_tests.rs"]
mod file_patch_tests;
