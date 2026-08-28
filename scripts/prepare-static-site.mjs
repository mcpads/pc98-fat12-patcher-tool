import { createHash } from 'node:crypto';
import {
  cp,
  copyFile,
  mkdir,
  readFile,
  readdir,
  rm,
  stat,
  writeFile,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const clientDirectory = path.join(projectRoot, 'dist', 'client');
const siteDirectory = path.join(projectRoot, 'dist', 'site');

function parsePackagePath(arguments_) {
  if (arguments_.length === 0) return null;
  if (arguments_.length !== 2 || arguments_[0] !== '--package') {
    throw new Error('usage: prepare-static-site.mjs [--package /path/to/patch.zip]');
  }
  return path.resolve(process.cwd(), arguments_[1]);
}

async function requireRegularFile(filePath, label) {
  const details = await stat(filePath).catch(() => null);
  if (!details?.isFile()) throw new Error(`${label} is missing: ${filePath}`);
  return details;
}

async function readPatcherConfig(filePath) {
  const value = JSON.parse(await readFile(filePath, 'utf8'));
  if (
    typeof value !== 'object'
    || value === null
    || Array.isArray(value)
    || Object.keys(value).join(',') !== 'package_url'
    || (value.package_url !== null && typeof value.package_url !== 'string')
  ) {
    throw new Error('patcher.json must contain only package_url as a string or null');
  }
  return value;
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

async function prepareHostedPackage(packagePath) {
  const details = await requireRegularFile(packagePath, 'patch package');
  if (path.extname(packagePath).toLowerCase() !== '.zip') {
    throw new Error('hosted patch package must use the .zip extension');
  }
  if (details.size === 0) throw new Error('hosted patch package is empty');

  const bytes = await readFile(packagePath);
  const digest = createHash('sha256').update(bytes).digest('hex');
  const filename = `package-${digest.slice(0, 12)}.zip`;
  const patchDirectory = path.join(siteDirectory, 'patch');
  await mkdir(patchDirectory, { recursive: true });
  await copyFile(packagePath, path.join(patchDirectory, filename));
  await writeFile(
    path.join(siteDirectory, 'patcher.json'),
    `${JSON.stringify({ package_url: `./patch/${filename}` }, null, 2)}\n`,
  );
  return filename;
}

const packagePath = parsePackagePath(process.argv.slice(2));
await requireRegularFile(path.join(clientDirectory, 'index.html'), 'static export index');
await requireRegularFile(path.join(clientDirectory, '404.html'), 'static export 404 page');
await requireRegularFile(path.join(clientDirectory, 'patcher.json'), 'static export configuration');

await rm(siteDirectory, { recursive: true, force: true });
await cp(clientDirectory, siteDirectory, { recursive: true });
await Promise.all([
  rm(path.join(siteDirectory, '.assetsignore'), { force: true }),
  rm(path.join(siteDirectory, '.vite'), { recursive: true, force: true }),
  rm(path.join(siteDirectory, '_headers'), { force: true }),
  rm(path.join(siteDirectory, 'vinext-client-entry-manifest.json'), { force: true }),
]);

const hostedPackage = packagePath ? await prepareHostedPackage(packagePath) : null;
await readPatcherConfig(path.join(siteDirectory, 'patcher.json'));
await requireReferencedAssets();

const files = await collectFiles(siteDirectory);
const wasmFiles = files.filter((filePath) => filePath.endsWith('.wasm'));
if (wasmFiles.length === 0) throw new Error('static output does not contain the patcher WebAssembly');
const totalBytes = (await Promise.all(files.map(async (filePath) => (await stat(filePath)).size)))
  .reduce((total, size) => total + size, 0);
const relativeSiteDirectory = path.relative(projectRoot, siteDirectory);

console.log(`static site ready: ${relativeSiteDirectory} (${files.length} files, ${totalBytes} bytes)`);
console.log(hostedPackage
  ? `hosted patch: patch/${hostedPackage}`
  : 'hosted patch: none (visitor selects a patch ZIP)');
