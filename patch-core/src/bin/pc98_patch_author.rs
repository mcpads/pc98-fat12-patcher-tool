use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use pc98_fat12_patcher_core::{
    PatchArtifact, PatchPackage, apply_patch_package, create_patch_package, create_patch_set,
    inspect_patch_artifact,
};
use retro_patch_utility::bps::{BpsLimits, inspect_patch_statistics};
use tempfile::NamedTempFile;

#[path = "pc98_patch_author/patch_set_plan.rs"]
mod patch_set_plan;

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
        ("create", [_, plan, source, content, output]) => {
            let plan_json = read_text(plan, "patch author plan")?;
            let source = read_bytes(source, "source HDM")?;
            let content = read_bytes(content, "content HDM")?;
            let package = create_patch_package(&plan_json, &source, &content)?;
            write_new(Path::new(output), &package)
        }
        ("create-set", [_, plan, output]) => {
            let plan = patch_set_plan::load_patch_set_plan(Path::new(plan))?;
            let patch_set = create_patch_set(&plan.id, &plan.title, plan.members)?;
            write_new(Path::new(output), &patch_set)
        }
        ("apply", [_, source, package, output]) => {
            let source = read_bytes(source, "source HDM")?;
            let package = read_bytes(package, "patch ZIP")?;
            let target = apply_patch_package(&source, &package)?;
            write_new(Path::new(output), &target)
        }
        ("inspect", [_, package]) => {
            let package = read_bytes(package, "patch ZIP")?;
            println!("package_bytes={}", package.len());
            match inspect_patch_artifact(&package)? {
                PatchArtifact::Single(contents) => {
                    println!("artifact_kind=single");
                    print_package(&contents, "")?;
                }
                PatchArtifact::Set(contents) => {
                    println!("artifact_kind=set");
                    println!("set_id={}", contents.manifest.id);
                    println!("members={}", contents.manifest.members.len());
                    for member in &contents.manifest.members {
                        println!("member={}", member.key);
                        println!("  label={}", member.label);
                        println!("  package_bytes={}", member.package_size);
                        println!("  package_sha256={}", member.package_sha256);
                        let nested = contents
                            .inspected_packages
                            .get(&member.key)
                            .expect("inspected package keys match manifest keys");
                        print_package(nested, "  ")?;
                    }
                }
            }
            Ok(())
        }
        _ => bail!(
            "usage:\n  pc98-patch-author create <plan.json> <source.hdm> <content.hdm> <output.zip>\n  pc98-patch-author create-set <set-plan.json> <output.zip>\n  pc98-patch-author apply <source.hdm> <single-patch.zip> <output.hdm>\n  pc98-patch-author inspect <patch.zip>"
        ),
    }
}

fn print_package(contents: &PatchPackage, indent: &str) -> Result<()> {
    println!("{indent}recipe_id={}", contents.recipe.id);
    println!(
        "{indent}output_filename={}",
        contents.recipe.output_filename
    );
    println!("{indent}source_bytes={}", contents.recipe.source.size);
    println!("{indent}source_sha256={}", contents.recipe.source.sha256);
    println!("{indent}target_bytes={}", contents.recipe.target.size);
    println!("{indent}target_sha256={}", contents.recipe.target.sha256);
    println!("{indent}file_patches={}", contents.patches.len());
    for file in &contents.recipe.assembly.placed_files {
        let patch_key = contents.recipe.patch_key_for(file)?;
        let Some(patch) = contents.patches.get(patch_key) else {
            continue;
        };
        let statistics = inspect_patch_statistics(
            patch,
            BpsLimits::new(
                patch.len(),
                file.source_size,
                file.target_size(),
                patch.len(),
                1_000_000,
            ),
        )?;
        println!("{indent}file={patch_key}");
        println!("{indent}  target_sfn={}", file.name);
        println!("{indent}  bps_bytes={}", patch.len());
        println!("{indent}  actions={}", statistics.action_count);
        println!(
            "{indent}  source_read_bytes={}",
            statistics.source_read_bytes
        );
        println!(
            "{indent}  target_read_bytes={}",
            statistics.target_read_bytes
        );
        println!(
            "{indent}  source_copy_bytes={}",
            statistics.source_copy_bytes
        );
        println!(
            "{indent}  target_copy_bytes={}",
            statistics.target_copy_bytes
        );
    }
    Ok(())
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
