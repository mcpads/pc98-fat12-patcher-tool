import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';

import initCore, {
  applyPatchPackage,
  classifyPatchArtifactInput,
  materializePatchArtifactMember,
  readPatchArtifactDefinition,
} from '../wasm/pc98_fat12_patcher_core.js';

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

const wasm = await readFile(
  new URL('../wasm/pc98_fat12_patcher_core_bg.wasm', import.meta.url),
);
await initCore({ module_or_path: wasm });

for (const directory of ['conformance', 'conformance/raw-sfn']) {
  const [source, patchPackage, target, manifestBytes] = await Promise.all([
    readFile(new URL(`../${directory}/source.hdm`, import.meta.url)),
    readFile(new URL(`../${directory}/package.zip`, import.meta.url)),
    readFile(new URL(`../${directory}/target.hdm`, import.meta.url)),
    readFile(new URL(`../${directory}/manifest.json`, import.meta.url)),
  ]);
  const manifest = JSON.parse(manifestBytes.toString('utf8'));

  assert.equal(sha256(source), manifest.source_sha256);
  assert.equal(sha256(patchPackage), manifest.package_sha256);
  assert.equal(sha256(target), manifest.target_sha256);
  const applied = applyPatchPackage(source, patchPackage);
  assert.deepEqual(Buffer.from(applied), target);

  process.stdout.write(
    `WASM conformance ${directory} target ${manifest.target_sha256}\n`,
  );
}

const [patchSet, setManifestBytes, asciiSource, asciiTarget, rawSource, rawTarget] = await Promise.all([
  readFile(new URL('../conformance/package-set/package.zip', import.meta.url)),
  readFile(new URL('../conformance/package-set/manifest.json', import.meta.url)),
  readFile(new URL('../conformance/source.hdm', import.meta.url)),
  readFile(new URL('../conformance/target.hdm', import.meta.url)),
  readFile(new URL('../conformance/raw-sfn/source.hdm', import.meta.url)),
  readFile(new URL('../conformance/raw-sfn/target.hdm', import.meta.url)),
]);
const setManifest = JSON.parse(setManifestBytes.toString('utf8'));
assert.equal(sha256(patchSet), setManifest.package_sha256);
const definition = JSON.parse(readPatchArtifactDefinition(patchSet));
assert.equal(definition.kind, 'set');
assert.deepEqual(definition.members.map((member) => member.key), ['ascii-disk', 'raw-sfn-disk']);
assert.deepEqual(JSON.parse(classifyPatchArtifactInput(rawSource, patchSet)), {
  kind: 'source',
  member_key: 'raw-sfn-disk',
});
assert.deepEqual(JSON.parse(classifyPatchArtifactInput(asciiTarget, patchSet)), {
  kind: 'target',
  member_key: 'ascii-disk',
});
assert.deepEqual(
  Buffer.from(materializePatchArtifactMember(asciiSource, patchSet, 'ascii-disk')),
  asciiTarget,
);
assert.deepEqual(
  Buffer.from(materializePatchArtifactMember(rawSource, patchSet, 'raw-sfn-disk')),
  rawTarget,
);
assert.deepEqual(
  Buffer.from(materializePatchArtifactMember(rawTarget, patchSet, 'raw-sfn-disk')),
  rawTarget,
);
process.stdout.write(`WASM conformance package set ${setManifest.package_sha256}\n`);
