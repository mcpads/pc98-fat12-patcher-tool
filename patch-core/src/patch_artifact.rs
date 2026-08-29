use std::io::Cursor;

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use zip::ZipArchive;

use crate::hash::sha256_hex;
use crate::limits::MAX_PATCH_SET_BYTES;
use crate::patch_package::{
    PatchPackage, RECIPE_ENTRY_NAME, apply_patch_package, collect_entry_names,
    inspect_patch_package,
};
use crate::patch_set::{PATCH_SET_ENTRY_NAME, PatchSet, inspect_patch_set};
use crate::recipe::PatchRecipe;

pub const SINGLE_ARTIFACT_MEMBER_KEY: &str = "single";

#[derive(Debug, Clone)]
pub enum PatchArtifact {
    Single(PatchPackage),
    Set(PatchSet),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchArtifactKind {
    Single,
    Set,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatchArtifactDefinition {
    pub kind: PatchArtifactKind,
    pub id: String,
    pub title: String,
    pub members: Vec<PatchArtifactMemberDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatchArtifactMemberDefinition {
    pub key: String,
    pub label: String,
    pub output_filename: String,
    pub source_size: usize,
    pub source_sha256: String,
    pub target_size: usize,
    pub target_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatchArtifactInputMatch {
    Source { member_key: String },
    Target { member_key: String },
    Unsupported,
}

pub fn inspect_patch_artifact(bytes: &[u8]) -> Result<PatchArtifact> {
    ensure!(
        bytes.len() <= MAX_PATCH_SET_BYTES,
        "patch artifact ZIP is too large: {} bytes exceeds {MAX_PATCH_SET_BYTES}",
        bytes.len()
    );
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("open patch artifact ZIP")?;
    let names = collect_entry_names(&mut archive)?;
    let recipe_count = names.get(RECIPE_ENTRY_NAME).copied().unwrap_or_default();
    let set_count = names.get(PATCH_SET_ENTRY_NAME).copied().unwrap_or_default();
    match (recipe_count, set_count) {
        (1, 0) => inspect_patch_package(bytes).map(PatchArtifact::Single),
        (0, 1) => inspect_patch_set(bytes).map(PatchArtifact::Set),
        (0, 0) => anyhow::bail!(
            "patch artifact ZIP is missing root entry {RECIPE_ENTRY_NAME} or {PATCH_SET_ENTRY_NAME}"
        ),
        _ => anyhow::bail!(
            "patch artifact ZIP must contain exactly one root marker, got {recipe_count} {RECIPE_ENTRY_NAME} entries and {set_count} {PATCH_SET_ENTRY_NAME} entries"
        ),
    }
}

pub fn patch_artifact_definition(bytes: &[u8]) -> Result<PatchArtifactDefinition> {
    definition_from_artifact(&inspect_patch_artifact(bytes)?)
}

pub fn classify_patch_artifact_input(
    input: &[u8],
    artifact_bytes: &[u8],
) -> Result<PatchArtifactInputMatch> {
    let artifact = inspect_patch_artifact(artifact_bytes)?;
    classify_input(&artifact, input)
}

pub fn materialize_patch_artifact_member(
    input: &[u8],
    artifact_bytes: &[u8],
    member_key: &str,
) -> Result<Vec<u8>> {
    let artifact = inspect_patch_artifact(artifact_bytes)?;
    match artifact {
        PatchArtifact::Single(contents) => {
            ensure!(
                member_key == SINGLE_ARTIFACT_MEMBER_KEY,
                "single patch artifact member key must be {SINGLE_ARTIFACT_MEMBER_KEY}"
            );
            materialize_package_input(input, artifact_bytes, &contents.recipe)
        }
        PatchArtifact::Set(contents) => {
            let package = contents
                .packages
                .get(member_key)
                .with_context(|| format!("patch set has no member {member_key}"))?;
            let recipe = &contents
                .inspected_packages
                .get(member_key)
                .expect("inspected package keys match package keys")
                .recipe;
            materialize_package_input(input, package, recipe)
        }
    }
}

fn definition_from_artifact(artifact: &PatchArtifact) -> Result<PatchArtifactDefinition> {
    match artifact {
        PatchArtifact::Single(contents) => Ok(PatchArtifactDefinition {
            kind: PatchArtifactKind::Single,
            id: contents.recipe.id.clone(),
            title: contents.recipe.title.clone(),
            members: vec![member_definition(
                SINGLE_ARTIFACT_MEMBER_KEY,
                &contents.recipe.title,
                &contents.recipe,
            )],
        }),
        PatchArtifact::Set(contents) => {
            let members = contents
                .manifest
                .members
                .iter()
                .map(|member| {
                    let recipe = &contents
                        .inspected_packages
                        .get(&member.key)
                        .expect("inspected package keys match manifest keys")
                        .recipe;
                    member_definition(&member.key, &member.label, recipe)
                })
                .collect();
            Ok(PatchArtifactDefinition {
                kind: PatchArtifactKind::Set,
                id: contents.manifest.id.clone(),
                title: contents.manifest.title.clone(),
                members,
            })
        }
    }
}

fn member_definition(
    key: &str,
    label: &str,
    recipe: &PatchRecipe,
) -> PatchArtifactMemberDefinition {
    PatchArtifactMemberDefinition {
        key: key.to_owned(),
        label: label.to_owned(),
        output_filename: recipe.output_filename.clone(),
        source_size: recipe.source.size,
        source_sha256: recipe.source.sha256.clone(),
        target_size: recipe.target.size,
        target_sha256: recipe.target.sha256.clone(),
    }
}

fn classify_input(artifact: &PatchArtifact, input: &[u8]) -> Result<PatchArtifactInputMatch> {
    let candidates = match artifact {
        PatchArtifact::Single(contents) => vec![(SINGLE_ARTIFACT_MEMBER_KEY, &contents.recipe)],
        PatchArtifact::Set(contents) => contents
            .manifest
            .members
            .iter()
            .map(|member| {
                (
                    member.key.as_str(),
                    &contents
                        .inspected_packages
                        .get(&member.key)
                        .expect("inspected package keys match manifest keys")
                        .recipe,
                )
            })
            .collect(),
    };
    let size_candidates = candidates
        .into_iter()
        .filter(|(_, recipe)| {
            input.len() == recipe.source.size || input.len() == recipe.target.size
        })
        .collect::<Vec<_>>();
    if size_candidates.is_empty() {
        return Ok(PatchArtifactInputMatch::Unsupported);
    }

    let input_sha256 = sha256_hex(input);
    for (member_key, recipe) in size_candidates {
        if input.len() == recipe.source.size && input_sha256 == recipe.source.sha256 {
            return Ok(PatchArtifactInputMatch::Source {
                member_key: member_key.to_owned(),
            });
        }
        if input.len() == recipe.target.size && input_sha256 == recipe.target.sha256 {
            return Ok(PatchArtifactInputMatch::Target {
                member_key: member_key.to_owned(),
            });
        }
    }
    Ok(PatchArtifactInputMatch::Unsupported)
}

fn materialize_package_input(
    input: &[u8],
    package: &[u8],
    recipe: &PatchRecipe,
) -> Result<Vec<u8>> {
    ensure!(
        input.len() == recipe.source.size || input.len() == recipe.target.size,
        "input image size does not match the member source or target"
    );
    let input_sha256 = sha256_hex(input);
    if input.len() == recipe.target.size && input_sha256 == recipe.target.sha256 {
        return Ok(input.to_vec());
    }
    ensure!(
        input.len() == recipe.source.size && input_sha256 == recipe.source.sha256,
        "input image SHA-256 does not match the member source or target"
    );
    apply_patch_package(input, package)
}

#[cfg(test)]
#[path = "patch_artifact_tests.rs"]
mod patch_artifact_tests;
