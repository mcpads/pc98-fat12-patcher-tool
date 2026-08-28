use super::*;
use crate::hash::sha256_hex;
use crate::recipe::{FileSource, FileTransform, PlacedFile};

fn patched_file(source: &[u8], target: &[u8]) -> PlacedFile {
    PlacedFile {
        name: "GAME.COM".to_owned(),
        source: FileSource::RootFile {
            name: "GAME.COM".to_owned(),
        },
        source_size: source.len(),
        source_sha256: sha256_hex(source),
        transform: FileTransform::Bps {
            target_size: target.len(),
            target_sha256: sha256_hex(target),
        },
    }
}

#[test]
fn file_patch_reproduces_the_declared_logical_file() {
    let source = b"original game bytes";
    let target = b"localized game bytes that may grow";
    let file = patched_file(source, target);
    let patch = create_file_patch("fixture", &file, source, target).unwrap();

    assert_eq!(
        apply_file_patch("fixture", &file, source, &patch).unwrap(),
        target
    );
}

#[test]
fn file_patch_is_bound_to_recipe_and_output_name() {
    let source = b"original";
    let target = b"localized";
    let file = patched_file(source, target);
    let patch = create_file_patch("fixture", &file, source, target).unwrap();

    assert!(
        inspect_file_patch("another-recipe", &file, &patch)
            .unwrap_err()
            .to_string()
            .contains("metadata")
    );
    let mut renamed = file;
    renamed.name = "OTHER.COM".to_owned();
    assert!(
        inspect_file_patch("fixture", &renamed, &patch)
            .unwrap_err()
            .to_string()
            .contains("metadata")
    );
}
