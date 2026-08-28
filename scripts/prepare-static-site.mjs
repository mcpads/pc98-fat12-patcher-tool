import {
  cp,
  readFile,
  readdir,
  rm,
  stat,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const clientDirectory = path.join(projectRoot, 'dist', 'client');
const siteDirectory = path.join(projectRoot, 'dist', 'site');

async function requireRegularFile(filePath, label) {
  const details = await stat(filePath).catch(() => null);
  if (!details?.isFile()) throw new Error(`${label} is missing: ${filePath}`);
  return details;
}

async function collectFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await collectFiles(entryPath));
    else if (entry.isFile()) files.push(entryPath);
    else throw new Error(`static output contains a non-regular entry: ${entryPath}`);
  }
  return files;
}

async function requireReferencedAssets() {
  const html = await readFile(path.join(siteDirectory, 'index.html'), 'utf8');
  const references = [...html.matchAll(/(?:href|src)="([^"#?]+)[^"#]*"/g)]
    .map((match) => match[1])
    .filter((reference) => reference.startsWith('/') && !reference.startsWith('//'));
  for (const reference of new Set(references)) {
    const relativePath = decodeURIComponent(reference).replace(/^\/+/, '');
    await requireRegularFile(path.join(siteDirectory, relativePath), `referenced asset ${reference}`);
  }
}

if (process.argv.length > 2) {
  throw new Error('prepare-static-site.mjs does not accept arguments');
}
await requireRegularFile(path.join(clientDirectory, 'index.html'), 'static export index');
await requireRegularFile(path.join(clientDirectory, '404.html'), 'static export 404 page');

await rm(siteDirectory, { recursive: true, force: true });
await cp(clientDirectory, siteDirectory, { recursive: true });
await Promise.all([
  rm(path.join(siteDirectory, '.assetsignore'), { force: true }),
  rm(path.join(siteDirectory, '.vite'), { recursive: true, force: true }),
  rm(path.join(siteDirectory, '_headers'), { force: true }),
  rm(path.join(siteDirectory, 'vinext-client-entry-manifest.json'), { force: true }),
]);

await requireReferencedAssets();

const files = await collectFiles(siteDirectory);
const wasmFiles = files.filter((filePath) => filePath.endsWith('.wasm'));
if (wasmFiles.length === 0) throw new Error('static output does not contain the patcher WebAssembly');
const totalBytes = (await Promise.all(files.map(async (filePath) => (await stat(filePath)).size)))
  .reduce((total, size) => total + size, 0);
const relativeSiteDirectory = path.relative(projectRoot, siteDirectory);

console.log(`static site ready: ${relativeSiteDirectory} (${files.length} files, ${totalBytes} bytes)`);
