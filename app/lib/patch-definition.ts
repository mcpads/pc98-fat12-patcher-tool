import { maximumPatchPackageBytes, readPatchPackageRecipe } from './patch-core';

export type RecipeSummary = {
  id: string;
  title: string;
  outputFilename: string;
  sourceSize: number;
  sourceSha256: string;
  targetSha256: string;
};

export type HostedPackage = {
  packageBytes: Uint8Array;
  packageName: string;
  summary: RecipeSummary;
};

type HostedConfig = {
  package_url: string | null;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function requireString(record: Record<string, unknown>, key: string) {
  const value = record[key];
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`레시피의 ${key} 값이 올바르지 않습니다.`);
  }
  return value;
}

function requireHash(record: Record<string, unknown>, key: string) {
  const value = requireString(record, key);
  if (!/^[0-9a-f]{64}$/.test(value)) {
    throw new Error(`레시피의 ${key} 값은 소문자 SHA-256이어야 합니다.`);
  }
  return value;
}

export function readRecipeSummary(recipeJson: string): RecipeSummary {
  let parsed: unknown;
  try {
    parsed = JSON.parse(recipeJson);
  } catch {
    throw new Error('레시피 JSON을 읽을 수 없습니다.');
  }
  if (!isRecord(parsed) || !isRecord(parsed.source) || !isRecord(parsed.target)) {
    throw new Error('레시피에 source와 target 정보가 없습니다.');
  }
  const sourceSize = parsed.source.size;
  if (!Number.isSafeInteger(sourceSize) || Number(sourceSize) <= 0) {
    throw new Error('레시피의 source.size 값이 올바르지 않습니다.');
  }
  const outputFilename = requireString(parsed, 'output_filename');
  if (outputFilename.includes('/') || outputFilename.includes('\\')) {
    throw new Error('출력 파일명에 경로를 넣을 수 없습니다.');
  }
  return {
    id: requireString(parsed, 'id'),
    title: requireString(parsed, 'title'),
    outputFilename,
    sourceSize: Number(sourceSize),
    sourceSha256: requireHash(parsed.source, 'sha256'),
    targetSha256: requireHash(parsed.target, 'sha256'),
  };
}

function parseHostedConfig(value: unknown): HostedConfig {
  if (!isRecord(value)) {
    throw new Error('patcher.json이 JSON 객체가 아닙니다.');
  }
  const keys = Object.keys(value).sort();
  if (keys.join(',') !== 'package_url') {
    throw new Error('patcher.json에는 package_url만 있어야 합니다.');
  }
  const packageUrl = value.package_url;
  if (packageUrl !== null && (typeof packageUrl !== 'string' || packageUrl.trim() === '')) {
    throw new Error('package_url은 비어 있지 않은 문자열 또는 null이어야 합니다.');
  }
  return { package_url: packageUrl };
}

async function fetchRequired(url: string, signal: AbortSignal) {
  const response = await fetch(url, { cache: 'no-store', signal });
  if (!response.ok) {
    throw new Error(`${url}을(를) 불러오지 못했습니다. HTTP ${response.status}`);
  }
  return response;
}

function filenameFromUrl(url: string, fallback: string) {
  const path = url.split(/[?#]/, 1)[0];
  const name = path.split('/').pop();
  return name || fallback;
}

async function readBoundedResponse(
  response: Response,
  maximumBytes: number,
  label: string,
) {
  const contentLength = response.headers.get('content-length');
  if (contentLength !== null) {
    const declaredBytes = Number(contentLength);
    if (!Number.isSafeInteger(declaredBytes) || declaredBytes < 0) {
      throw new Error(`${label}의 Content-Length가 올바르지 않습니다.`);
    }
    if (declaredBytes > maximumBytes) {
      throw new Error(`${label}이 허용 크기를 초과합니다.`);
    }
  }
  if (!response.body) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.length > maximumBytes) throw new Error(`${label}이 허용 크기를 초과합니다.`);
    return bytes;
  }

  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let totalBytes = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    totalBytes += value.byteLength;
    if (totalBytes > maximumBytes) {
      await reader.cancel();
      throw new Error(`${label}이 허용 크기를 초과합니다.`);
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(totalBytes);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

export async function loadHostedPackage(signal: AbortSignal) {
  const configResponse = await fetchRequired('./patcher.json', signal);
  const config = parseHostedConfig(await configResponse.json());
  if (config.package_url === null) {
    return null;
  }
  const packageResponse = await fetchRequired(config.package_url, signal);
  const packageBytes = await readBoundedResponse(
    packageResponse,
    await maximumPatchPackageBytes(),
    '호스팅 패치 ZIP',
  );
  const recipeJson = await readPatchPackageRecipe(packageBytes);
  return {
    packageBytes,
    packageName: filenameFromUrl(config.package_url, 'patch.zip'),
    summary: readRecipeSummary(recipeJson),
  } satisfies HostedPackage;
}
