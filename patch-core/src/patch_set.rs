use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Cursor, Write};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::hash::{require_sha256, sha256_hex, validate_sha256};
use crate::limits::{
    MAX_PATCH_PACKAGE_BYTES, MAX_PATCH_SET_BYTES, MAX_PATCH_SET_MANIFEST_BYTES,
    MAX_PATCH_SET_MEMBERS, MAX_ZIP_ENTRIES,
};
use crate::patch_package::{
    PatchPackage, collect_entry_names, inspect_patch_package, read_entry, require_single_entry,
};

pub const PATCH_SET_FORMAT: &str = "retrogame-patcher-pc98-fat12-package-set";
pub const PATCH_SET_ENTRY_NAME: &str = "patch-set.json";
pub const PACKAGE_SET_DIRECTORY: &str = "packages/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchSetManifest {
    pub format: String,
    pub id: String,
    pub title: String,
    pub members: Vec<PatchSetMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchSetMember {
    pub key: String,
    pub label: String,
    pub package_size: usize,
    pub package_sha256: String,
}

#[derive(Debug, Clone)]
pub struct PatchSet {
    pub manifest_json: String,
    pub manifest: PatchSetManifest,
    pub packages: BTreeMap<String, Vec<u8>>,
    pub inspected_packages: BTreeMap<String, PatchPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchSetPackageInput {
    pub key: String,
    pub label: String,
    pub package: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct UnsupportedPatchSetFormat;

impl fmt::Display for UnsupportedPatchSetFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsupported patch set format; expected {PATCH_SET_FORMAT}"
        )
    }
}

impl std::error::Error for UnsupportedPatchSetFormat {}

pub fn create_patch_set(
    id: &str,
    title: &str,
    members: Vec<PatchSetPackageInput>,
) -> Result<Vec<u8>> {
    let manifest = PatchSetManifest {
        format: PATCH_SET_FORMAT.to_owned(),
        id: id.to_owned(),
        title: title.to_owned(),
        members: members
            .iter()
            .map(|member| PatchSetMember {
                key: member.key.clone(),
                label: member.label.clone(),
                package_size: member.package.len(),
                package_sha256: sha256_hex(&member.package),
            })
            .collect(),
    };
    manifest.validate()?;

    let packages = members
        .into_iter()
        .map(|member| (member.key, member.package))
        .collect::<BTreeMap<_, _>>();
    validate_member_packages(&manifest, &packages)?;
    let manifest_json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    let patch_set = write_patch_set(manifest_json.as_bytes(), &manifest, &packages)?;

    let inspected = inspect_patch_set(&patch_set).context("verify newly created patch set ZIP")?;
    ensure!(
        inspected.manifest == manifest,
        "new patch set ZIP changed its generated manifest"
    );
    ensure!(
        inspected.packages == packages,
        "new patch set ZIP changed a nested patch package"
    );
    Ok(patch_set)
}

pub fn inspect_patch_set(package_set: &[u8]) -> Result<PatchSet> {
    ensure!(
        package_set.len() <= MAX_PATCH_SET_BYTES,
        "patch set ZIP is too large: {} bytes exceeds {MAX_PATCH_SET_BYTES}",
        package_set.len()
    );
    let mut archive = ZipArchive::new(Cursor::new(package_set)).context("open patch set ZIP")?;
    ensure!(
        archive.len() <= MAX_ZIP_ENTRIES,
        "patch set ZIP has too many entries: {} exceeds {MAX_ZIP_ENTRIES}",
        archive.len()
    );
    let names = collect_entry_names(&mut archive)?;
    require_single_entry(&names, PATCH_SET_ENTRY_NAME)?;
    ensure!(
        !names.contains_key(crate::patch_package::RECIPE_ENTRY_NAME),
        "patch set ZIP cannot contain both {PATCH_SET_ENTRY_NAME} and {}",
        crate::patch_package::RECIPE_ENTRY_NAME
    );

    let manifest_bytes = read_entry(
        &mut archive,
        PATCH_SET_ENTRY_NAME,
        MAX_PATCH_SET_MANIFEST_BYTES as u64,
    )?;
    let manifest_json = String::from_utf8(manifest_bytes).context("patch-set.json is not UTF-8")?;
    let manifest = parse_patch_set_manifest(&manifest_json)?;
    let expected_entries = manifest
        .members
        .iter()
        .map(|member| (member.key.clone(), package_entry_name(&member.key)))
        .collect::<BTreeMap<_, _>>();
    require_package_entries(&names, &expected_entries)?;

    let mut total_package_bytes = 0usize;
    let mut packages = BTreeMap::new();
    for member in &manifest.members {
        let entry_name = expected_entries
            .get(&member.key)
            .expect("entry names were derived from patch set members");
        let package = read_entry(&mut archive, entry_name, MAX_PATCH_PACKAGE_BYTES as u64)?;
        total_package_bytes = total_package_bytes
            .checked_add(package.len())
            .context("nested patch package size overflow")?;
        ensure!(
            total_package_bytes <= MAX_PATCH_SET_BYTES,
            "nested patch packages expand to {total_package_bytes} bytes, exceeding {MAX_PATCH_SET_BYTES}"
        );
        packages.insert(member.key.clone(), package);
    }

    let inspected_packages = validate_member_packages(&manifest, &packages)?;
    Ok(PatchSet {
        manifest_json,
        manifest,
        packages,
        inspected_packages,
    })
}

pub fn parse_patch_set_manifest(json: &str) -> Result<PatchSetManifest> {
    ensure!(
        json.len() <= MAX_PATCH_SET_MANIFEST_BYTES,
        "patch set manifest is too large: {} bytes exceeds {MAX_PATCH_SET_MANIFEST_BYTES}",
        json.len()
    );
    let value: serde_json::Value =
        serde_json::from_str(json).context("parse patch set manifest JSON")?;
    let format = value
        .as_object()
        .and_then(|object| object.get("format"))
        .and_then(serde_json::Value::as_str);
    if format != Some(PATCH_SET_FORMAT) {
        return Err(UnsupportedPatchSetFormat.into());
    }
    let manifest: PatchSetManifest =
        serde_json::from_value(value).context("parse patch set manifest JSON")?;
    manifest.validate()?;
    Ok(manifest)
}

impl PatchSetManifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format == PATCH_SET_FORMAT,
            "patch set format must be {PATCH_SET_FORMAT}"
        );
        ensure!(!self.id.trim().is_empty(), "patch set id cannot be empty");
        ensure!(
            !self.title.trim().is_empty(),
            "patch set title cannot be empty"
        );
        ensure!(
            (1..=MAX_PATCH_SET_MEMBERS).contains(&self.members.len()),
            "patch set must contain 1..={MAX_PATCH_SET_MEMBERS} members"
        );
        let mut keys = BTreeSet::new();
        let mut declared_package_bytes = 0usize;
        for member in &self.members {
            validate_member_key(&member.key)?;
            ensure!(
                keys.insert(member.key.as_str()),
                "duplicate patch set member key: {}",
                member.key
            );
            ensure!(
                !member.label.trim().is_empty(),
                "patch set member {} label cannot be empty",
                member.key
            );
            ensure!(
                (1..=MAX_PATCH_PACKAGE_BYTES).contains(&member.package_size),
                "patch set member {} package size must be 1..={MAX_PATCH_PACKAGE_BYTES}",
                member.key
            );
            validate_sha256(
                &member.package_sha256,
                &format!("patch set member {} package", member.key),
            )?;
            declared_package_bytes = declared_package_bytes
                .checked_add(member.package_size)
                .context("patch set declared package size overflow")?;
        }
        ensure!(
            declared_package_bytes <= MAX_PATCH_SET_BYTES,
            "patch set declares {declared_package_bytes} bytes of nested packages, exceeding {MAX_PATCH_SET_BYTES}"
        );
        Ok(())
    }
}

pub fn package_entry_name(key: &str) -> String {
    format!("{PACKAGE_SET_DIRECTORY}{key}.zip")
}

fn validate_member_packages(
    manifest: &PatchSetManifest,
    packages: &BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<String, PatchPackage>> {
    let expected_keys = manifest
        .members
        .iter()
        .map(|member| member.key.as_str())
        .collect::<BTreeSet<_>>();
    let actual_keys = packages.keys().map(String::as_str).collect::<BTreeSet<_>>();
    ensure!(
        actual_keys == expected_keys,
        "patch set package keys differ: expected {expected_keys:?}, got {actual_keys:?}"
    );

    let mut image_identities = BTreeMap::<String, (String, &'static str)>::new();
    let mut inspected = BTreeMap::new();
    for member in &manifest.members {
        let package = packages
            .get(&member.key)
            .expect("package keys were compared with the manifest");
        ensure!(
            package.len() == member.package_size,
            "patch set member {} package size mismatch: expected {}, got {}",
            member.key,
            member.package_size,
            package.len()
        );
        require_sha256(
            package,
            &member.package_sha256,
            &format!("patch set member {} package", member.key),
        )?;
        let contents = inspect_patch_package(package)
            .with_context(|| format!("inspect patch set member {}", member.key))?;
        register_image_identity(
            &mut image_identities,
            &contents.recipe.source.sha256,
            &member.key,
            "source",
        )?;
        register_image_identity(
            &mut image_identities,
            &contents.recipe.target.sha256,
            &member.key,
            "target",
        )?;
        inspected.insert(member.key.clone(), contents);
    }
    Ok(inspected)
}

fn register_image_identity(
    identities: &mut BTreeMap<String, (String, &'static str)>,
    sha256: &str,
    member_key: &str,
    role: &'static str,
) -> Result<()> {
    if let Some((existing_key, existing_role)) = identities.get(sha256) {
        anyhow::bail!(
            "patch set image SHA-256 is ambiguous: member {existing_key} {existing_role} and member {member_key} {role} both use {sha256}"
        );
    }
    identities.insert(sha256.to_owned(), (member_key.to_owned(), role));
    Ok(())
}

fn write_patch_set(
    manifest_json: &[u8],
    manifest: &PatchSetManifest,
    packages: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
    let output = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(output);
    let common_options = SimpleFileOptions::default()
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    let manifest_options = common_options
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6));
    archive
        .start_file(PATCH_SET_ENTRY_NAME, manifest_options)
        .context("start patch-set.json in patch set ZIP")?;
    archive
        .write_all(manifest_json)
        .context("write patch-set.json to patch set ZIP")?;

    let package_options = common_options.compression_method(CompressionMethod::Stored);
    for member in &manifest.members {
        let package = packages
            .get(&member.key)
            .expect("packages were validated against the manifest");
        let entry_name = package_entry_name(&member.key);
        archive
            .start_file(&entry_name, package_options)
            .with_context(|| format!("start {entry_name} in patch set ZIP"))?;
        archive
            .write_all(package)
            .with_context(|| format!("write {entry_name} to patch set ZIP"))?;
    }
    let bytes = archive
        .finish()
        .context("finish patch set ZIP")?
        .into_inner();
    ensure!(
        bytes.len() <= MAX_PATCH_SET_BYTES,
        "patch set ZIP is too large: {} bytes exceeds {MAX_PATCH_SET_BYTES}",
        bytes.len()
    );
    Ok(bytes)
}

fn require_package_entries(
    names: &BTreeMap<String, usize>,
    expected_entries: &BTreeMap<String, String>,
) -> Result<()> {
    let expected = expected_entries.values().cloned().collect::<BTreeSet<_>>();
    let actual = names
        .keys()
        .filter(|name| name.starts_with(PACKAGE_SET_DIRECTORY))
        .cloned()
        .collect::<BTreeSet<_>>();
    ensure!(
        actual == expected,
        "patch set package entries differ: expected {expected:?}, got {actual:?}"
    );
    for name in expected {
        require_single_entry(names, &name)?;
    }
    Ok(())
}

fn validate_member_key(key: &str) -> Result<()> {
    ensure!(
        (1..=64).contains(&key.len())
            && key
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && key
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') }),
        "patch set member key must be 1..=64 safe ASCII bytes and start with an alphanumeric character: {key:?}"
    );
    Ok(())
}

#[cfg(test)]
#[path = "patch_set_tests.rs"]
mod patch_set_tests;
