use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use pc98_fat12_patcher_core::{
    apply_patch_package, build_baseline, create_patch_package, inspect_patch_package,
    inspect_patch_statistics, parse_recipe,
};
use tempfile::NamedTempFile;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let command = arguments
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match (command, arguments.as_slice()) {
        ("baseline", [_, recipe, source, output]) => {
            let recipe = read_recipe(recipe)?;
            let source = read_bytes(source, "source HDM")?;
            let baseline = build_baseline(&recipe, &source)?;
            write_new(Path::new(output), &baseline)
        }
        ("create", [_, recipe, source, target, output]) => {
            let recipe_json = read_text(recipe, "patch recipe")?;
            let source = read_bytes(source, "source HDM")?;
            let target = read_bytes(target, "target HDM")?;
            let package = create_patch_package(&recipe_json, &source, &target)?;
            write_new(Path::new(output), &package)
        }
        ("apply", [_, source, package, output]) => {
            let source = read_bytes(source, "source HDM")?;
            let package = read_bytes(package, "patch ZIP")?;
            let target = apply_patch_package(&source, &package)?;
            write_new(Path::new(output), &target)
        }
        ("inspect", [_, package]) => {
            let package = read_bytes(package, "patch ZIP")?;
            let contents = inspect_patch_package(&package)?;
            let statistics = inspect_patch_statistics(&contents.patch)?;
            println!("package_bytes={}", package.len());
            println!("recipe_id={}", contents.recipe.id);
            println!("output_filename={}", contents.recipe.output_filename);
            println!("bps_bytes={}", contents.patch.len());
            println!("source_bytes={}", contents.recipe.source.size);
            println!("target_bytes={}", contents.recipe.target.size);
            println!("actions={}", statistics.action_count);
            println!("source_read_bytes={}", statistics.source_read_bytes);
            println!("target_read_bytes={}", statistics.target_read_bytes);
            println!("source_copy_bytes={}", statistics.source_copy_bytes);
            println!("target_copy_bytes={}", statistics.target_copy_bytes);
            Ok(())
        }
        _ => bail!(
            "usage:\n  pc98-patch-author baseline <recipe.json> <source.hdm> <output.hdm>\n  pc98-patch-author create <recipe.json> <source.hdm> <target.hdm> <output.zip>\n  pc98-patch-author apply <source.hdm> <patch.zip> <output.hdm>\n  pc98-patch-author inspect <patch.zip>"
        ),
    }
}

fn read_recipe(path: &OsString) -> Result<pc98_fat12_patcher_core::PatchRecipe> {
    let json = read_text(path, "patch recipe")?;
    parse_recipe(&json)
}

fn read_text(path: &OsString, label: &str) -> Result<String> {
    let path = Path::new(path);
    fs::read_to_string(path).with_context(|| format!("read {label} {}", path.display()))
}

fn read_bytes(path: &OsString, label: &str) -> Result<Vec<u8>> {
    let path = Path::new(path);
    fs::read(path).with_context(|| format!("read {label} {}", path.display()))
}

fn write_new(output: &Path, bytes: &[u8]) -> Result<()> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create output directory {}", parent.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("create temporary output in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .context("write temporary output")?;
    temporary
        .as_file()
        .sync_all()
        .context("flush temporary output")?;
    temporary.persist_noclobber(output).map_err(|error| {
        anyhow::anyhow!(
            "publish output without overwriting {}: {}",
            output.display(),
            error.error
        )
    })?;
    println!("wrote {} bytes to {}", bytes.len(), output.display());
    Ok(())
}
