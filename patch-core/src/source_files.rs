use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::fat12::read_root_files;
use crate::hash::require_sha256;
use crate::lha_sfx::extract_mz_lha_members;
use crate::recipe::{ExactFile, FileSource, PatchRecipe};

pub(crate) fn resolve_assembly_files(
    source: &[u8],
    recipe: &PatchRecipe,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let required_root_names = required_root_names(recipe);
    let required_archive_members = required_archive_members(recipe)?;
    let root_files = read_root_files(source, recipe.source.mount_policy, &required_root_names)?;
    verify_retained_files(&root_files, &recipe.assembly.retained_files)?;

    let mut archives = BTreeMap::new();
    let mut placed = BTreeMap::new();
    for file in &recipe.assembly.placed_files {
        let bytes = match &file.source {
            FileSource::RootFile { name } => root_files
                .get(name)
                .with_context(|| format!("required root source file is missing: {name}"))?
                .clone(),
            FileSource::MzLhaMember { container, member } => {
                if !archives.contains_key(container) {
                    let executable = root_files.get(container).with_context(|| {
                        format!("required MZ+LHA container is missing: {container}")
                    })?;
                    let members = required_archive_members.get(container).with_context(|| {
                        format!("recipe has no required members for LHA container {container}")
                    })?;
                    archives.insert(
                        container.clone(),
                        extract_mz_lha_members(executable, members)?,
                    );
                }
                archives[container]
                    .get(member)
                    .with_context(|| format!("{container} is missing LHA member {member}"))?
                    .clone()
            }
        };
        ensure!(
            bytes.len() == file.size,
            "{} size mismatch: expected {}, got {}",
            file.name,
            file.size,
            bytes.len()
        );
        require_sha256(&bytes, &file.sha256, &file.name)?;
        placed.insert(file.name.clone(), bytes);
    }
    Ok(placed)
}

fn required_archive_members(
    recipe: &PatchRecipe,
) -> Result<BTreeMap<String, BTreeMap<String, usize>>> {
    let mut archives = BTreeMap::<String, BTreeMap<String, usize>>::new();
    for file in &recipe.assembly.placed_files {
        let FileSource::MzLhaMember { container, member } = &file.source else {
            continue;
        };
        let previous = archives
            .entry(container.clone())
            .or_default()
            .insert(member.clone(), file.size);
        ensure!(
            previous.is_none_or(|size| size == file.size),
            "LHA member {container}:{member} has conflicting expected sizes"
        );
    }
    Ok(archives)
}

fn required_root_names(recipe: &PatchRecipe) -> BTreeSet<String> {
    recipe
        .assembly
        .retained_files
        .iter()
        .map(|file| file.name.clone())
        .chain(
            recipe
                .assembly
                .placed_files
                .iter()
                .map(|file| match &file.source {
                    FileSource::RootFile { name } => name.clone(),
                    FileSource::MzLhaMember { container, .. } => container.clone(),
                }),
        )
        .collect()
}

fn verify_retained_files(
    root_files: &BTreeMap<String, Vec<u8>>,
    retained_files: &[ExactFile],
) -> Result<()> {
    for expected in retained_files {
        let bytes = root_files
            .get(&expected.name)
            .with_context(|| format!("retained file is missing: {}", expected.name))?;
        ensure!(
            bytes.len() == expected.size,
            "{} size mismatch: expected {}, got {}",
            expected.name,
            expected.size,
            bytes.len()
        );
        require_sha256(bytes, &expected.sha256, &expected.name)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "source_files_tests.rs"]
mod source_files_tests;
