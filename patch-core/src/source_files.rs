use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::fat12::read_root_files;
use crate::hash::require_sha256;
use crate::lha_sfx::extract_mz_lha_members;
use crate::recipe::{
    ExactFile, FileSource, PackageFormat, PatchPlan, PatchRecipe, PlacedFile, PlannedFile,
    SourceImage,
};

pub(crate) fn resolve_plan_files(
    source: &[u8],
    plan: &PatchPlan,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let format = plan.package_format()?;
    resolve_files(
        source,
        &plan.source,
        &plan.assembly.retained_files,
        &plan.assembly.placed_files,
        format,
    )
}

pub(crate) fn resolve_recipe_files(
    source: &[u8],
    recipe: &PatchRecipe,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let format = recipe.package_format()?;
    resolve_files(
        source,
        &recipe.source,
        &recipe.assembly.retained_files,
        &recipe.assembly.placed_files,
        format,
    )
}

trait Placement {
    fn patch_key(&self, format: PackageFormat) -> Result<&str>;
    fn source(&self) -> &FileSource;
    fn source_size(&self) -> usize;
    fn source_sha256(&self) -> &str;
}

impl Placement for PlannedFile {
    fn patch_key(&self, format: PackageFormat) -> Result<&str> {
        self.effective_patch_key(format)
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
    fn patch_key(&self, format: PackageFormat) -> Result<&str> {
        self.effective_patch_key(format)
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
    format: PackageFormat,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let required_root_names = required_root_names(retained_files, placed_files)?;
    let required_archive_members = required_archive_members(placed_files)?;
    let root_files = read_root_files(source, source_image.mount_policy, &required_root_names)?;
    verify_retained_files(&root_files, retained_files)?;

    let mut archives = BTreeMap::new();
    let mut placed = BTreeMap::new();
    for file in placed_files {
        let patch_key = file.patch_key(format)?;
        let bytes = match file.source() {
            FileSource::RootFile { name } => root_files
                .get(&name.raw_bytes("root source file")?)
                .with_context(|| format!("required root source file is missing: {name}"))?
                .clone(),
            FileSource::MzLhaMember { container, member } => {
                let container_raw = container.raw_bytes("LHA container")?;
                let archive = match archives.entry(container_raw) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let executable = root_files.get(&container_raw).with_context(|| {
                            format!("required MZ+LHA container is missing: {container}")
                        })?;
                        let members =
                            required_archive_members
                                .get(&container_raw)
                                .with_context(|| {
                                    format!(
                                        "plan has no required members for LHA container {container}"
                                    )
                                })?;
                        entry.insert(extract_mz_lha_members(executable, members)?)
                    }
                };
                archive
                    .get(member)
                    .with_context(|| format!("{container} is missing LHA member {member}"))?
                    .clone()
            }
            FileSource::Empty => Vec::new(),
        };
        ensure!(
            bytes.len() == file.source_size(),
            "{} source size mismatch: expected {}, got {}",
            patch_key,
            file.source_size(),
            bytes.len()
        );
        require_sha256(&bytes, file.source_sha256(), &format!("{patch_key} source"))?;
        placed.insert(patch_key.to_owned(), bytes);
    }
    Ok(placed)
}

fn required_archive_members<T: Placement>(
    placed_files: &[T],
) -> Result<BTreeMap<[u8; 11], BTreeMap<crate::fat_name::LhaMemberName, usize>>> {
    let mut archives = BTreeMap::<[u8; 11], BTreeMap<crate::fat_name::LhaMemberName, usize>>::new();
    for file in placed_files {
        let FileSource::MzLhaMember { container, member } = file.source() else {
            continue;
        };
        let previous = archives
            .entry(container.raw_bytes("LHA container")?)
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
) -> Result<BTreeSet<[u8; 11]>> {
    let mut names = BTreeSet::new();
    for file in retained_files {
        names.insert(file.name.raw_bytes("retained file")?);
    }
    for file in placed_files {
        match file.source() {
            FileSource::RootFile { name } => {
                names.insert(name.raw_bytes("root source file")?);
            }
            FileSource::MzLhaMember { container, .. } => {
                names.insert(container.raw_bytes("LHA container")?);
            }
            FileSource::Empty => {}
        }
    }
    Ok(names)
}

fn verify_retained_files(
    root_files: &BTreeMap<[u8; 11], Vec<u8>>,
    retained_files: &[ExactFile],
) -> Result<()> {
    for expected in retained_files {
        let raw_name = expected.name.raw_bytes("retained file")?;
        let bytes = root_files
            .get(&raw_name)
            .with_context(|| format!("retained file is missing: {}", expected.name))?;
        ensure!(
            bytes.len() == expected.size,
            "{} size mismatch: expected {}, got {}",
            expected.name,
            expected.size,
            bytes.len()
        );
        require_sha256(bytes, &expected.sha256, &expected.name.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "source_files_tests.rs"]
mod source_files_tests;
