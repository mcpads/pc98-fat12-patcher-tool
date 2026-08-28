use std::io::{Cursor, Read, Write};

use anyhow::{Context, Result, bail, ensure};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::limits::{MAX_BPS_BYTES, MAX_PATCH_PACKAGE_BYTES, MAX_RECIPE_BYTES, MAX_ZIP_ENTRIES};
use crate::pipeline::{build_baseline, create_recipe_patch, require_recipe_patch};
use crate::recipe::{PatchRecipe, parse_recipe};

pub const RECIPE_ENTRY_NAME: &str = "recipe.json";
pub const BPS_ENTRY_NAME: &str = "patch.bps";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchPackage {
    pub recipe_json: String,
    pub recipe: PatchRecipe,
    pub patch: Vec<u8>,
}

pub fn create_patch_package(recipe_json: &str, source: &[u8], target: &[u8]) -> Result<Vec<u8>> {
    let recipe = parse_recipe(recipe_json)?;
    let patch = create_recipe_patch(&recipe, source, target)?;
    let package = write_patch_package(recipe_json.as_bytes(), &patch)?;
    inspect_patch_package(&package).context("verify newly created patch ZIP")?;
    Ok(package)
}

pub fn inspect_patch_package(package: &[u8]) -> Result<PatchPackage> {
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
    require_conventional_entries(&mut archive)?;

    let recipe_bytes = read_entry(
        &mut archive,
        RECIPE_ENTRY_NAME,
        u64::try_from(MAX_RECIPE_BYTES).context("recipe size limit does not fit ZIP limits")?,
    )?;
    let recipe_json = String::from_utf8(recipe_bytes).context("recipe.json is not UTF-8")?;
    let recipe = parse_recipe(&recipe_json).context("parse recipe.json from patch ZIP")?;

    let maximum_patch_bytes = u64::try_from(recipe.target.size)
        .context("target size does not fit ZIP limits")?
        .checked_mul(2)
        .and_then(|size| size.checked_add(MAX_RECIPE_BYTES as u64))
        .context("BPS size limit overflow")?;
    ensure!(
        maximum_patch_bytes <= MAX_BPS_BYTES as u64,
        "BPS size limit exceeds the application budget"
    );
    let patch = read_entry(&mut archive, BPS_ENTRY_NAME, maximum_patch_bytes)?;
    require_recipe_patch(&recipe, &patch).context("validate patch.bps against recipe.json")?;

    Ok(PatchPackage {
        recipe_json,
        recipe,
        patch,
    })
}

pub fn apply_patch_package(source: &[u8], package: &[u8]) -> Result<Vec<u8>> {
    let contents = inspect_patch_package(package)?;
    let baseline = build_baseline(&contents.recipe, source)?;
    crate::pipeline::apply_recipe_patch(&contents.recipe, &baseline, &contents.patch)
}

fn write_patch_package(recipe_json: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    let output = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6))
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);

    archive
        .start_file(RECIPE_ENTRY_NAME, options)
        .context("start recipe.json in patch ZIP")?;
    archive
        .write_all(recipe_json)
        .context("write recipe.json to patch ZIP")?;
    archive
        .start_file(BPS_ENTRY_NAME, options)
        .context("start patch.bps in patch ZIP")?;
    archive
        .write_all(patch)
        .context("write patch.bps to patch ZIP")?;

    Ok(archive.finish().context("finish patch ZIP")?.into_inner())
}

fn require_conventional_entries(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<()> {
    let mut has_recipe = false;
    let mut has_patch = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("read patch ZIP entry {index}"))?;
        match entry.name() {
            RECIPE_ENTRY_NAME if !has_recipe => has_recipe = true,
            BPS_ENTRY_NAME if !has_patch => has_patch = true,
            RECIPE_ENTRY_NAME => bail!("patch ZIP contains duplicate {RECIPE_ENTRY_NAME}"),
            BPS_ENTRY_NAME => bail!("patch ZIP contains duplicate {BPS_ENTRY_NAME}"),
            _ => {}
        }
    }
    ensure!(
        has_recipe,
        "patch ZIP is missing root entry {RECIPE_ENTRY_NAME}"
    );
    ensure!(
        has_patch,
        "patch ZIP is missing root entry {BPS_ENTRY_NAME}"
    );
    Ok(())
}

fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    maximum_size: u64,
) -> Result<Vec<u8>> {
    let entry = archive
        .by_name(name)
        .with_context(|| format!("open {name} from patch ZIP"))?;
    ensure!(!entry.is_dir(), "patch ZIP entry {name} is a directory");
    ensure!(!entry.encrypted(), "patch ZIP entry {name} is encrypted");
    ensure!(
        matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ),
        "patch ZIP entry {name} uses an unsupported compression method"
    );
    ensure!(
        entry.size() <= maximum_size,
        "patch ZIP entry {name} is too large: {} bytes exceeds {maximum_size}",
        entry.size()
    );
    let announced_size = usize::try_from(entry.size())
        .with_context(|| format!("{name} size does not fit memory"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(announced_size)
        .with_context(|| format!("reserve {name} buffer"))?;
    let read_limit = maximum_size
        .checked_add(1)
        .context("patch ZIP extraction limit overflow")?;
    entry
        .take(read_limit)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {name} from patch ZIP"))?;
    ensure!(
        bytes.len() <= maximum_size as usize,
        "patch ZIP entry {name} expands past its {maximum_size}-byte limit"
    );
    ensure!(
        bytes.len() == announced_size,
        "patch ZIP entry {name} announced {announced_size} bytes but yielded {}",
        bytes.len()
    );
    Ok(bytes)
}

#[cfg(test)]
#[path = "patch_package_tests.rs"]
mod patch_package_tests;
