use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::fat12::read_root_files;
use crate::hash::require_sha256;
use crate::lha_sfx::extract_mz_lha_members;
use crate::recipe::{
    ExactFile, FileSource, PatchPlan, PatchRecipe, PlacedFile, PlannedFile, SourceImage,
};

pub(crate) fn resolve_plan_files(
    source: &[u8],
    plan: &PatchPlan,
) -> Result<BTreeMap<String, Vec<u8>>> {
    resolve_files(
        source,
        &plan.source,
        &plan.assembly.retained_files,
        &plan.assembly.placed_files,
    )
}

pub(crate) fn resolve_recipe_files(
    source: &[u8],
    recipe: &PatchRecipe,
) -> Result<BTreeMap<String, Vec<u8>>> {
    resolve_files(
        source,
        &recipe.source,
        &recipe.assembly.retained_files,
        &recipe.assembly.placed_files,
    )
}

trait Placement {
    fn output_name(&self) -> &str;
    fn source(&self) -> &FileSource;
    fn source_size(&self) -> usize;
    fn source_sha256(&self) -> &str;
}

impl Placement for PlannedFile {
    fn output_name(&self) -> &str {
        &self.name
    }

    fn source(&self) -> &FileSource {
        &self.source
    }

    fn source_size(&self) -> usize {
        self.source_size
    }

    fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
}

impl Placement for PlacedFile {
    fn output_name(&self) -> &str {
        &self.name
    }

    fn source(&self) -> &FileSource {
        &self.source
    }

    fn source_size(&self) -> usize {
        self.source_size
    }

    fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
}

fn resolve_files<T: Placement>(
    source: &[u8],
    source_image: &SourceImage,
    retained_files: &[ExactFile],
    placed_files: &[T],
) -> Result<BTreeMap<String, Vec<u8>>> {
    let required_root_names = required_root_names(retained_files, placed_files);
    let required_archive_members = required_archive_members(placed_files)?;
    let root_files = read_root_files(source, source_image.mount_policy, &required_root_names)?;
    verify_retained_files(&root_files, retained_files)?;

    let mut archives = BTreeMap::new();
    let mut placed = BTreeMap::new();
    for file in placed_files {
        let bytes = match file.source() {
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
                        format!("plan has no required members for LHA container {container}")
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
            FileSource::Empty => Vec::new(),
        };
        ensure!(
            bytes.len() == file.source_size(),
            "{} source size mismatch: expected {}, got {}",
            file.output_name(),
            file.source_size(),
            bytes.len()
        );
        require_sha256(
            &bytes,
            file.source_sha256(),
            &format!("{} source", file.output_name()),
        )?;
        placed.insert(file.output_name().to_owned(), bytes);
    }
    Ok(placed)
}

fn required_archive_members<T: Placement>(
    placed_files: &[T],
) -> Result<BTreeMap<String, BTreeMap<String, usize>>> {
    let mut archives = BTreeMap::<String, BTreeMap<String, usize>>::new();
    for file in placed_files {
        let FileSource::MzLhaMember { container, member } = file.source() else {
            continue;
        };
        let previous = archives
            .entry(container.clone())
            .or_default()
            .insert(member.clone(), file.source_size());
        ensure!(
            previous.is_none_or(|size| size == file.source_size()),
            "LHA member {container}:{member} has conflicting expected sizes"
        );
    }
    Ok(archives)
}

fn required_root_names<T: Placement>(
    retained_files: &[ExactFile],
    placed_files: &[T],
) -> BTreeSet<String> {
    retained_files
        .iter()
        .map(|file| file.name.clone())
        .chain(placed_files.iter().filter_map(|file| match file.source() {
            FileSource::RootFile { name } => Some(name.clone()),
            FileSource::MzLhaMember { container, .. } => Some(container.clone()),
            FileSource::Empty => None,
        }))
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
