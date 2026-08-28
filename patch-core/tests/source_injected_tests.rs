use std::fs;

use pc98_fat12_patcher_core::{
    apply_patch_package, build_baseline, create_patch_package, inspect_patch_package, parse_recipe,
};

fn required_path(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must point to a local test input"))
}

#[test]
#[ignore = "requires user-owned source and target HDM files"]
fn exact_source_rebuilds_the_declared_baseline_and_target() {
    let recipe_json = fs::read_to_string(required_path("PC98_PATCH_RECIPE"))
        .expect("read source-injected recipe");
    let source = fs::read(required_path("PC98_PATCH_SOURCE")).expect("read source HDM");
    let target = fs::read(required_path("PC98_PATCH_TARGET")).expect("read target HDM");
    let recipe = parse_recipe(&recipe_json).expect("parse source-injected recipe");

    let baseline = build_baseline(&recipe, &source).expect("rebuild canonical baseline");
    let package = create_patch_package(&recipe_json, &source, &target)
        .expect("create conventional patch ZIP");
    let contents = inspect_patch_package(&package).expect("inspect conventional patch ZIP");
    let applied = apply_patch_package(&source, &package).expect("apply conventional patch ZIP");

    assert_eq!(applied, target);
    assert_eq!(contents.recipe, recipe);
    eprintln!(
        "source={} bytes, baseline={} bytes, target={} bytes, bps={} bytes, zip={} bytes",
        source.len(),
        baseline.len(),
        target.len(),
        contents.patch.len(),
        package.len()
    );
}
