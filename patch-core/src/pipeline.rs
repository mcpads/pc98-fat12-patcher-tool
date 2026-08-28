use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::fat12::{assemble_image, read_root_files, require_fat12_structure, require_geometry};
use crate::file_patch::{apply_file_patch, create_file_patch};
use crate::hash::{require_sha256, sha256_hex};
use crate::recipe::{
    AssemblyRecipe, FileTransform, PatchPlan, PatchRecipe, PlacedFile, PlannedTransform,
    TargetImage,
};
use crate::source_files::{resolve_plan_files, resolve_recipe_files};

pub(crate) struct CreatedPackageContents {
    pub recipe_json: String,
    pub recipe: PatchRecipe,
    pub patches: BTreeMap<String, Vec<u8>>,
    pub target: Vec<u8>,
}

pub(crate) fn create_package_contents(
    plan: PatchPlan,
    source: &[u8],
    content_image: &[u8],
) -> Result<CreatedPackageContents> {
    plan.validate()?;
    let format = plan.package_format()?;
    require_source_image(&plan.source, source)?;
    require_content_image(&plan, content_image)?;

    let source_files = resolve_plan_files(source, &plan)?;
    let output_names = plan
        .assembly
        .placed_files
        .iter()
        .map(|file| file.name.raw_bytes("placed file name"))
        .collect::<Result<BTreeSet<_>>>()?;
    let content_files = read_root_files(content_image, plan.source.mount_policy, &output_names)
        .context("read logical target files from content image")?;

    let mut placed_files = Vec::with_capacity(plan.assembly.placed_files.len());
    let mut target_files = BTreeMap::new();
    for planned in &plan.assembly.placed_files {
        let patch_key = planned.effective_patch_key(format)?;
        let source_file = source_files
            .get(patch_key)
            .with_context(|| format!("resolved source set is missing {patch_key}"))?;
        let content_file = content_files
            .get(&planned.name.raw_bytes("placed file name")?)
            .with_context(|| format!("content image is missing {}", planned.name))?;
        let transform = match planned.transform {
            PlannedTransform::Copy => {
                ensure!(
                    content_file == source_file,
                    "{} is declared copy but content image changes it",
                    planned.name
                );
                FileTransform::Copy
            }
            PlannedTransform::Bps => FileTransform::Bps {
                target_size: content_file.len(),
                target_sha256: sha256_hex(content_file),
            },
        };
        placed_files.push(PlacedFile {
            patch_key: planned.patch_key.clone(),
            name: planned.name.clone(),
            source: planned.source.clone(),
            source_size: planned.source_size,
            source_sha256: planned.source_sha256.clone(),
            transform,
        });
        target_files.insert(patch_key.to_owned(), content_file.clone());
    }

    let placements = placed_files
        .iter()
        .map(|file| {
            Ok((
                file.effective_patch_key(format)?.to_owned(),
                file.name.clone(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let target = assemble_image(
        source,
        &plan.source,
        &plan.assembly.retained_files,
        &placements,
        &target_files,
    )
    .context("assemble canonical target HDM")?;
    require_fat12_structure(&target, &plan.source.geometry, plan.source.mount_policy)?;

    let recipe = PatchRecipe {
        format: format.identifier().to_owned(),
        id: plan.id,
        title: plan.title,
        output_filename: plan.output_filename,
        source: plan.source,
        assembly: AssemblyRecipe {
            retained_files: plan.assembly.retained_files,
            placed_files,
        },
        target: TargetImage {
            size: target.len(),
            sha256: sha256_hex(&target),
        },
    };
    recipe.validate()?;

    let mut patches = BTreeMap::new();
    for file in &recipe.assembly.placed_files {
        if !matches!(file.transform, FileTransform::Bps { .. }) {
            continue;
        }
        let patch_key = file.effective_patch_key(format)?;
        let source_file = source_files
            .get(patch_key)
            .with_context(|| format!("resolved source set is missing {patch_key}"))?;
        let target_file = target_files
            .get(patch_key)
            .with_context(|| format!("target file set is missing {patch_key}"))?;
        patches.insert(
            patch_key.to_owned(),
            create_file_patch(&recipe, file, source_file, target_file)
                .with_context(|| format!("create {patch_key} BPS"))?,
        );
    }
    let recipe_json = format!("{}\n", serde_json::to_string_pretty(&recipe)?);
    Ok(CreatedPackageContents {
        recipe_json,
        recipe,
        patches,
        target,
    })
}

pub(crate) fn apply_package_contents(
    recipe: &PatchRecipe,
    patches: &BTreeMap<String, Vec<u8>>,
    source: &[u8],
) -> Result<Vec<u8>> {
    recipe.validate()?;
    let format = recipe.package_format()?;
    require_source_image(&recipe.source, source)?;
    let source_files = resolve_recipe_files(source, recipe)?;
    let mut target_files = BTreeMap::new();
    for file in &recipe.assembly.placed_files {
        let patch_key = file.effective_patch_key(format)?;
        let source_file = source_files
            .get(patch_key)
            .with_context(|| format!("resolved source set is missing {patch_key}"))?;
        let target_file = match &file.transform {
            FileTransform::Copy => source_file.clone(),
            FileTransform::Bps { .. } => {
                let patch = patches
                    .get(patch_key)
                    .with_context(|| format!("patch package is missing {patch_key} BPS"))?;
                apply_file_patch(recipe, file, source_file, patch)
                    .with_context(|| format!("apply {patch_key} BPS"))?
            }
        };
        target_files.insert(patch_key.to_owned(), target_file);
    }
    let placements = recipe
        .assembly
        .placed_files
        .iter()
        .map(|file| {
            Ok((
                file.effective_patch_key(format)?.to_owned(),
                file.name.clone(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let target = assemble_image(
        source,
        &recipe.source,
        &recipe.assembly.retained_files,
        &placements,
        &target_files,
    )
    .context("assemble patched HDM")?;
    ensure!(
        target.len() == recipe.target.size,
        "target image size mismatch: expected {}, got {}",
        recipe.target.size,
        target.len()
    );
    require_sha256(&target, &recipe.target.sha256, "target image")?;
    require_fat12_structure(&target, &recipe.source.geometry, recipe.source.mount_policy)?;
    Ok(target)
}

fn require_source_image(source_profile: &crate::recipe::SourceImage, source: &[u8]) -> Result<()> {
    ensure!(
        source.len() == source_profile.size,
        "source image size mismatch: expected {}, got {}",
        source_profile.size,
        source.len()
    );
    require_sha256(source, &source_profile.sha256, "source image")?;
    require_geometry(source, &source_profile.geometry)
}

fn require_content_image(plan: &PatchPlan, content_image: &[u8]) -> Result<()> {
    ensure!(
        content_image.len() == plan.source.size,
        "content image size mismatch: expected {}, got {}",
        plan.source.size,
        content_image.len()
    );
    require_geometry(content_image, &plan.source.geometry)?;
    require_fat12_structure(
        content_image,
        &plan.source.geometry,
        plan.source.mount_policy,
    )
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod pipeline_tests;
