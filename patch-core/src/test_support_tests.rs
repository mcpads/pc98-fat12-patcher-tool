use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, format_volume};

use crate::fat_name::FatShortName;
use crate::fat12::assemble_image;
use crate::hash::sha256_hex;
use crate::recipe::{
    ExactFile, Fat12Geometry, FileSource, MountPolicy, PatchPlan, PlannedAssemblyRecipe,
    PlannedFile, PlannedTransform, SourceImage,
};

pub(crate) const FIXTURE_SIZE: usize = 1_474_560;

pub(crate) fn fixture_geometry() -> Fat12Geometry {
    Fat12Geometry {
        bytes_per_sector: 512,
        sectors_per_cluster: 1,
        reserved_sectors: 1,
        fat_count: 2,
        root_entries: 224,
        total_sectors: 2_880,
        media_descriptor: 0xf0,
        sectors_per_fat: 9,
        sectors_per_track: 18,
        heads: 2,
    }
}

pub(crate) fn fixture_image(files: &[(&str, &[u8])], include_directory: bool) -> Vec<u8> {
    let mut image = vec![0_u8; FIXTURE_SIZE];
    let options = FormatVolumeOptions::new()
        .bytes_per_sector(512)
        .bytes_per_cluster(512)
        .total_sectors(2_880)
        .fat_type(FatType::Fat12)
        .max_root_dir_entries(224)
        .fats(2)
        .media(0xf0)
        .sectors_per_track(18)
        .heads(2);
    format_volume(Cursor::new(image.as_mut_slice()), options).unwrap();
    {
        let filesystem =
            FileSystem::new(Cursor::new(image.as_mut_slice()), FsOptions::new()).unwrap();
        let root = filesystem.root_dir();
        for (name, bytes) in files {
            let mut file = root.create_file(name).unwrap();
            file.write_all(bytes).unwrap();
        }
        if include_directory {
            let directory = root.create_dir("JUNK").unwrap();
            let mut file = directory.create_file("OLD.TXT").unwrap();
            file.write_all(b"remove me").unwrap();
        }
        drop(root);
        filesystem.unmount().unwrap();
    }
    image
}

pub(crate) fn direct_root_plan(
    source: &[u8],
    retained_name: &str,
    retained_bytes: &[u8],
    input_name: &str,
    output_name: &str,
    input_bytes: &[u8],
) -> PatchPlan {
    PatchPlan {
        format: None,
        id: "fixture-patch".to_owned(),
        title: "Fixture Patch".to_owned(),
        output_filename: "fixture-patched.hdm".to_owned(),
        source: SourceImage {
            size: source.len(),
            sha256: sha256_hex(source),
            geometry: fixture_geometry(),
            mount_policy: MountPolicy::Standard,
        },
        assembly: PlannedAssemblyRecipe {
            retained_files: vec![ExactFile {
                name: FatShortName::ascii(retained_name),
                size: retained_bytes.len(),
                sha256: sha256_hex(retained_bytes),
            }],
            placed_files: vec![PlannedFile {
                patch_key: None,
                name: FatShortName::ascii(output_name),
                source: FileSource::RootFile {
                    name: FatShortName::ascii(input_name),
                },
                source_size: input_bytes.len(),
                source_sha256: sha256_hex(input_bytes),
                transform: PlannedTransform::Bps,
            }],
        },
    }
}

pub(crate) fn content_image(source: &[u8], plan: &PatchPlan, files: &[(&str, &[u8])]) -> Vec<u8> {
    let placed = files
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), (*bytes).to_vec()))
        .collect::<BTreeMap<_, _>>();
    let format = plan.package_format().unwrap();
    let placements = plan
        .assembly
        .placed_files
        .iter()
        .map(|file| {
            (
                file.effective_patch_key(format).unwrap().to_owned(),
                file.name.clone(),
            )
        })
        .collect::<Vec<_>>();
    assemble_image(
        source,
        &plan.source,
        &plan.assembly.retained_files,
        &placements,
        &placed,
    )
    .unwrap()
}

pub(crate) fn patch_fixture(marker: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let retained = vec![b's', b'y', b's', marker];
    let payload = vec![b'o', b'l', b'd', marker];
    let localized = vec![b'n', b'e', b'w', marker];
    let source = fixture_image(
        &[
            ("SYSTEM.SYS", retained.as_slice()),
            ("INSTALL.BIN", payload.as_slice()),
        ],
        false,
    );
    let mut plan = direct_root_plan(
        &source,
        "SYSTEM.SYS",
        &retained,
        "INSTALL.BIN",
        "GAME.COM",
        &payload,
    );
    plan.id = format!("fixture-{marker}");
    plan.title = format!("Fixture {marker}");
    plan.output_filename = format!("fixture-{marker}-patched.hdm");
    let target = content_image(&source, &plan, &[("GAME.COM", &localized)]);
    let plan_json = serde_json::to_string_pretty(&plan).unwrap();
    let package = crate::create_patch_package(&plan_json, &source, &target).unwrap();
    (source, package, target)
}
