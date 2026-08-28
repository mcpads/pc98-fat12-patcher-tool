use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::bps;
use crate::fat12::{assemble_baseline, require_fat12_structure, require_geometry};
use crate::hash::require_sha256;
use crate::recipe::PatchRecipe;
use crate::source_files::resolve_assembly_files;

const PATCH_METADATA_FORMAT: &str = "pc98-fat12-patcher";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchMetadata {
    format: String,
    recipe_id: String,
    source_sha256: String,
    baseline_sha256: String,
    target_sha256: String,
}

pub fn build_baseline(recipe: &PatchRecipe, source: &[u8]) -> Result<Vec<u8>> {
    recipe.validate()?;
    ensure!(
        source.len() == recipe.source.size,
        "source image size mismatch: expected {}, got {}",
        recipe.source.size,
        source.len()
    );
    require_sha256(source, &recipe.source.sha256, "source image")?;
    require_geometry(source, &recipe.source.geometry)?;
    let placed_files = resolve_assembly_files(source, recipe)?;
    let baseline = assemble_baseline(source, recipe, &placed_files)?;
    require_sha256(
        &baseline,
        &recipe.assembly.baseline_sha256,
        "canonical baseline image",
    )?;
    Ok(baseline)
}

pub fn create_recipe_patch(recipe: &PatchRecipe, source: &[u8], target: &[u8]) -> Result<Vec<u8>> {
    let baseline = build_baseline(recipe, source)?;
    require_target(recipe, target)?;
    let metadata = encode_metadata(recipe)?;
    let patch = bps::create_patch(&baseline, target, &metadata)?;
    let reapplied = apply_recipe_patch(recipe, &baseline, &patch)?;
    ensure!(
        reapplied == target,
        "new BPS did not reproduce the target byte-for-byte"
    );
    Ok(patch)
}

pub fn apply_recipe_patch(recipe: &PatchRecipe, baseline: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    recipe.validate()?;
    ensure!(
        baseline.len() == recipe.source.size,
        "baseline image size mismatch: expected {}, got {}",
        recipe.source.size,
        baseline.len()
    );
    require_sha256(
        baseline,
        &recipe.assembly.baseline_sha256,
        "canonical baseline image",
    )?;
    let info = require_recipe_patch(recipe, patch)?;
    ensure!(
        info.source_size == baseline.len(),
        "BPS source size does not match the canonical baseline"
    );
    let applied = bps::apply_patch(baseline, patch)?;
    require_target(recipe, &applied.target)?;
    Ok(applied.target)
}

pub(crate) fn require_recipe_patch(recipe: &PatchRecipe, patch: &[u8]) -> Result<bps::PatchInfo> {
    recipe.validate()?;
    let info = bps::inspect_patch(patch)?;
    bps::inspect_patch_statistics(patch).context("validate BPS action stream")?;
    ensure!(
        info.source_size == recipe.source.size,
        "BPS source size does not match the recipe source image size"
    );
    ensure!(
        info.target_size == recipe.target.size,
        "BPS target size does not match the recipe target"
    );
    validate_metadata(recipe, &info.metadata)?;
    Ok(info)
}

fn require_target(recipe: &PatchRecipe, target: &[u8]) -> Result<()> {
    ensure!(
        target.len() == recipe.target.size,
        "target image size mismatch: expected {}, got {}",
        recipe.target.size,
        target.len()
    );
    require_sha256(target, &recipe.target.sha256, "target image")?;
    require_fat12_structure(target, &recipe.source.geometry, recipe.source.mount_policy)
}

fn encode_metadata(recipe: &PatchRecipe) -> Result<Vec<u8>> {
    let metadata = PatchMetadata {
        format: PATCH_METADATA_FORMAT.to_owned(),
        recipe_id: recipe.id.clone(),
        source_sha256: recipe.source.sha256.clone(),
        baseline_sha256: recipe.assembly.baseline_sha256.clone(),
        target_sha256: recipe.target.sha256.clone(),
    };
    serde_json::to_vec(&metadata).context("serialize BPS patch metadata")
}

fn validate_metadata(recipe: &PatchRecipe, metadata: &[u8]) -> Result<()> {
    let metadata: PatchMetadata =
        serde_json::from_slice(metadata).context("parse BPS patch metadata")?;
    ensure!(
        metadata.format == PATCH_METADATA_FORMAT,
        "BPS metadata format is not {PATCH_METADATA_FORMAT}"
    );
    ensure!(metadata.recipe_id == recipe.id, "BPS recipe id mismatch");
    ensure!(
        metadata.source_sha256 == recipe.source.sha256,
        "BPS source identity mismatch"
    );
    ensure!(
        metadata.baseline_sha256 == recipe.assembly.baseline_sha256,
        "BPS baseline identity mismatch"
    );
    ensure!(
        metadata.target_sha256 == recipe.target.sha256,
        "BPS target identity mismatch"
    );
    Ok(())
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod pipeline_tests;
