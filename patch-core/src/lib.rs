mod fat12;
mod fat_name;
mod file_patch;
mod hash;
mod lha_sfx;
mod limits;
mod patch_package;
mod pipeline;
mod recipe;
mod source_files;
mod web_error;

#[cfg(test)]
#[path = "test_support_tests.rs"]
mod test_support;

pub use fat_name::{FatShortName, LhaMemberName};
pub use patch_package::{
    PATCH_DIRECTORY, PatchPackage, RECIPE_ENTRY_NAME, apply_patch_package, create_patch_package,
    inspect_patch_package, patch_entry_name,
};
pub use recipe::{
    LEGACY_PACKAGE_FORMAT, PACKAGE_FORMAT, PatchPlan, PatchRecipe, parse_plan, parse_recipe,
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = readPatchPackageRecipe))]
pub fn read_patch_package_recipe_for_web(package: &[u8]) -> Result<String, String> {
    inspect_patch_package(package)
        .map(|contents| contents.recipe_json)
        .map_err(web_error::describe_package_selection_failure)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = maximumPatchPackageBytes))]
pub fn maximum_patch_package_bytes_for_web() -> usize {
    limits::MAX_PATCH_PACKAGE_BYTES
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = applyPatchPackage))]
pub fn apply_patch_package_for_web(source: &[u8], package: &[u8]) -> Result<Vec<u8>, String> {
    apply_patch_package(source, package).map_err(display_error)
}

fn display_error(error: anyhow::Error) -> String {
    format!("{error:#}")
}
