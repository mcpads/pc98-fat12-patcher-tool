import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';

import initCore, { applyPatchPackage } from '../wasm/pc98_fat12_patcher_core.js';

const [source, patchPackage, target, manifestBytes, wasm] = await Promise.all([
  readFile(new URL('../conformance/source.hdm', import.meta.url)),
  readFile(new URL('../conformance/package.zip', import.meta.url)),
  readFile(new URL('../conformance/target.hdm', import.meta.url)),
  readFile(new URL('../conformance/manifest.json', import.meta.url)),
  readFile(new URL('../wasm/pc98_fat12_patcher_core_bg.wasm', import.meta.url)),
]);
const manifest = JSON.parse(manifestBytes.toString('utf8'));
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

assert.equal(sha256(source), manifest.source_sha256);
assert.equal(sha256(patchPackage), manifest.package_sha256);
assert.equal(sha256(target), manifest.target_sha256);
await initCore({ module_or_path: wasm });
const applied = applyPatchPackage(source, patchPackage);
assert.deepEqual(Buffer.from(applied), target);

process.stdout.write(`WASM conformance target ${manifest.target_sha256}\n`);
