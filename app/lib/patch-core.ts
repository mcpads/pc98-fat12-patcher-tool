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

export async function materializePatchMember(
  input: Uint8Array,
  patchArtifact: Uint8Array,
  memberKey: string,
) {
  const core = await loadCore();
  return core.materializePatchArtifactMember(input, patchArtifact, memberKey);
}

export async function readPatchArtifactDefinition(patchArtifact: Uint8Array) {
  const core = await loadCore();
  return core.readPatchArtifactDefinition(patchArtifact);
}

export async function classifyPatchInput(input: Uint8Array, patchArtifact: Uint8Array) {
  const core = await loadCore();
  return core.classifyPatchArtifactInput(input, patchArtifact);
}

export async function maximumPatchArtifactBytes() {
  const core = await loadCore();
  return core.maximumPatchArtifactBytes();
}
