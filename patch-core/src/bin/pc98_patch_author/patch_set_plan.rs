use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pc98_fat12_patcher_core::PatchSetPackageInput;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchSetAuthorPlan {
    id: String,
    title: String,
    members: Vec<PatchSetAuthorMember>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchSetAuthorMember {
    key: String,
    label: String,
    package_path: PathBuf,
}

pub(crate) struct LoadedPatchSetPlan {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) members: Vec<PatchSetPackageInput>,
}

pub(crate) fn load_patch_set_plan(path: &Path) -> Result<LoadedPatchSetPlan> {
    let json = fs::read_to_string(path)
        .with_context(|| format!("read patch set author plan {}", path.display()))?;
    let plan: PatchSetAuthorPlan =
        serde_json::from_str(&json).context("parse patch set author plan JSON")?;
    let plan_directory = path
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let members = plan
        .members
        .into_iter()
        .map(|member| {
            let package_path = if member.package_path.is_absolute() {
                member.package_path
            } else {
                plan_directory.join(member.package_path)
            };
            let package = fs::read(&package_path).with_context(|| {
                format!(
                    "read patch set member {} package {}",
                    member.key,
                    package_path.display()
                )
            })?;
            Ok(PatchSetPackageInput {
                key: member.key,
                label: member.label,
                package,
            })
        })
        .collect::<Result<_>>()?;
    Ok(LoadedPatchSetPlan {
        id: plan.id,
        title: plan.title,
        members,
    })
}
