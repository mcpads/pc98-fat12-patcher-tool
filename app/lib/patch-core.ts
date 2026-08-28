type PatcherCore = typeof import('../../wasm/pc98_fat12_patcher_core.js');

let initializedCore: Promise<PatcherCore> | undefined;

async function loadCore() {
  if (!initializedCore) {
    initializedCore = import('../../wasm/pc98_fat12_patcher_core.js').then(async (core) => {
      await core.default();
      return core;
    });
  }
  return initializedCore;
}

export async function makePatchedImage(
  source: Uint8Array,
  patchPackage: Uint8Array,
) {
  const core = await loadCore();
  return core.applyPatchPackage(source, patchPackage);
}

export async function readPatchPackageRecipe(patchPackage: Uint8Array) {
  const core = await loadCore();
  return core.readPatchPackageRecipe(patchPackage);
}

export async function maximumPatchPackageBytes() {
  const core = await loadCore();
  return core.maximumPatchPackageBytes();
}
