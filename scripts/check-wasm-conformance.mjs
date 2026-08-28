import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';

import initCore, { applyPatchPackage } from '../wasm/pc98_fat12_patcher_core.js';

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
