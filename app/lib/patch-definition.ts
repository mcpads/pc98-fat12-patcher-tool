export type RecipeSummary = {
  id: string;
  title: string;
  outputFilename: string;
  sourceSize: number;
  sourceSha256: string;
  targetSha256: string;
};

const PACKAGE_FORMATS = new Set([
  'retrogame-patcher-pc98-fat12-file-bps',
  'retrogame-patcher-pc98-fat12-raw-sfn-file-bps',
]);

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
  if (typeof parsed.format !== 'string' || !PACKAGE_FORMATS.has(parsed.format)) {
    throw new Error('지원하지 않는 패치 ZIP 형식입니다.');
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
