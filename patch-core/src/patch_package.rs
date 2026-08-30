use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use anyhow::{Context, Result, ensure};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::file_patch::inspect_file_patch;
use crate::limits::{MAX_BPS_BYTES, MAX_PATCH_PACKAGE_BYTES, MAX_RECIPE_BYTES, MAX_ZIP_ENTRIES};
use crate::patch_set::PATCH_SET_ENTRY_NAME;
use crate::pipeline::{apply_package_contents, create_package_contents};
use crate::recipe::{FileTransform, PatchRecipe, parse_plan, parse_recipe};

pub const RECIPE_ENTRY_NAME: &str = "recipe.json";
pub const PATCH_DIRECTORY: &str = "patches/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchPackage {
    pub recipe_json: String,
    pub recipe: PatchRecipe,
    pub patches: BTreeMap<String, Vec<u8>>,
}

pub fn create_patch_package(
    plan_json: &str,
    source: &[u8],
    content_image: &[u8],
) -> Result<Vec<u8>> {
    let plan = parse_plan(plan_json)?;
    let contents = create_package_contents(plan, source, content_image)?;
    let package = write_patch_package(contents.recipe_json.as_bytes(), &contents.patches)?;
    let inspected = inspect_patch_package(&package).context("verify newly created patch ZIP")?;
    ensure!(
        inspected.recipe == contents.recipe,
        "new patch ZIP changed its generated recipe"
    );
    let reapplied = apply_package_contents(&inspected.recipe, &inspected.patches, source)
        .context("self-apply newly created patch ZIP")?;
    ensure!(
        reapplied == contents.target,
        "new patch ZIP did not reproduce its canonical target byte-for-byte"
    );
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
    let recipe = parse_recipe(&recipe_json).context("parse recipe.json from patch ZIP")?;
    let format = recipe.package_format()?;

    let expected_entries = recipe
        .assembly
        .placed_files
        .iter()
        .filter(|file| matches!(file.transform, FileTransform::Bps { .. }))
        .map(|file| {
            let patch_key = file.effective_patch_key(format)?.to_owned();
            Ok((patch_key.clone(), patch_entry_name(&patch_key)))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    require_patch_entries(&names, &expected_entries)?;

    let mut extracted_bytes = 0usize;
    let mut patches = BTreeMap::new();
    for file in &recipe.assembly.placed_files {
        if !matches!(file.transform, FileTransform::Bps { .. }) {
            continue;
        }
        let patch_key = file.effective_patch_key(format)?;
        let entry_name = expected_entries
            .get(patch_key)
            .expect("entry names were derived from placed files");
        let patch = read_entry(&mut archive, entry_name, MAX_BPS_BYTES as u64)?;
        extracted_bytes = extracted_bytes
            .checked_add(patch.len())
            .context("total BPS payload size overflow")?;
        ensure!(
            extracted_bytes <= MAX_BPS_BYTES,
            "BPS payloads expand to {extracted_bytes} bytes, exceeding {MAX_BPS_BYTES}"
        );
        inspect_file_patch(&recipe, file, &patch)
            .with_context(|| format!("validate {entry_name}"))?;
        patches.insert(patch_key.to_owned(), patch);
    }

    Ok(PatchPackage {
        recipe_json,
        recipe,
        patches,
    })
}

pub fn apply_patch_package(source: &[u8], package: &[u8]) -> Result<Vec<u8>> {
    let contents = inspect_patch_package(package)?;
    apply_package_contents(&contents.recipe, &contents.patches, source)
}

pub fn patch_entry_name(patch_key: &str) -> String {
    format!("{PATCH_DIRECTORY}{patch_key}.bps")
}

pub(crate) fn write_patch_package(
    recipe_json: &[u8],
    patches: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
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
    for (name, patch) in patches {
        let entry_name = patch_entry_name(name);
        archive
            .start_file(&entry_name, options)
            .with_context(|| format!("start {entry_name} in patch ZIP"))?;
        archive
            .write_all(patch)
            .with_context(|| format!("write {entry_name} to patch ZIP"))?;
    }

    Ok(archive.finish().context("finish patch ZIP")?.into_inner())
}

pub(crate) fn collect_entry_names(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<BTreeMap<String, usize>> {
    let mut names = BTreeMap::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("read patch ZIP entry {index}"))?;
        let count = names.entry(entry.name().to_owned()).or_insert(0usize);
        *count = count
            .checked_add(1)
            .context("patch ZIP duplicate count overflow")?;
    }
    Ok(names)
}

pub(crate) fn require_single_entry(names: &BTreeMap<String, usize>, name: &str) -> Result<()> {
    match names.get(name).copied().unwrap_or_default() {
        1 => Ok(()),
        0 => anyhow::bail!("patch ZIP is missing root entry {name}"),
        count => anyhow::bail!("patch ZIP contains {count} entries named {name}"),
    }
}

pub(crate) fn require_patch_entries(
    names: &BTreeMap<String, usize>,
    expected_entries: &BTreeMap<String, String>,
) -> Result<()> {
    let expected = expected_entries.values().cloned().collect::<BTreeSet<_>>();
    let actual = names
        .keys()
        .filter(|name| name.starts_with(PATCH_DIRECTORY))
        .cloned()
        .collect::<BTreeSet<_>>();
    ensure!(
        actual == expected,
        "patch ZIP file-patch entries differ: expected {expected:?}, got {actual:?}"
    );
    for name in expected {
        require_single_entry(names, &name)?;
    }
    Ok(())
}

pub(crate) fn read_entry(
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
