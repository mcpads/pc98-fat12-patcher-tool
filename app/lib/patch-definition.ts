export type PatchArtifactMember = {
  key: string;
  label: string;
  outputFilename: string;
  sourceSize: number;
  sourceSha256: string;
  targetSize: number;
  targetSha256: string;
};

export type PatchArtifactDefinition = {
  kind: 'single' | 'set';
  id: string;
  title: string;
  members: PatchArtifactMember[];
};

export type PatchArtifactInputMatch =
  | { kind: 'source'; memberKey: string }
  | { kind: 'target'; memberKey: string }
  | { kind: 'unsupported' };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requireString(record: Record<string, unknown>, key: string) {
  const value = record[key];
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`패치 정의의 ${key} 값이 올바르지 않습니다.`);
  }
  return value;
}

function requireSize(record: Record<string, unknown>, key: string) {
  const value = record[key];
  if (!Number.isSafeInteger(value) || Number(value) <= 0) {
    throw new Error(`패치 정의의 ${key} 값이 올바르지 않습니다.`);
  }
  return Number(value);
}

function requireHash(record: Record<string, unknown>, key: string) {
  const value = requireString(record, key);
  if (!/^[0-9a-f]{64}$/.test(value)) {
    throw new Error(`패치 정의의 ${key} 값은 소문자 SHA-256이어야 합니다.`);
  }
  return value;
}

function parseJson(json: string, label: string): unknown {
  try {
    return JSON.parse(json);
  } catch {
    throw new Error(`${label} JSON을 읽을 수 없습니다.`);
  }
}

export function readPatchArtifactDefinition(json: string): PatchArtifactDefinition {
  const parsed = parseJson(json, '패치 정의');
  if (!isRecord(parsed) || !Array.isArray(parsed.members)) {
    throw new Error('패치 정의에 필수 매체 목록이 없습니다.');
  }
  if (parsed.kind !== 'single' && parsed.kind !== 'set') {
    throw new Error('지원하지 않는 패치 정의 종류입니다.');
  }
  if (parsed.members.length < 1 || parsed.members.length > 32) {
    throw new Error('패치 정의의 필수 매체 수가 올바르지 않습니다.');
  }
  const keys = new Set<string>();
  const members = parsed.members.map((value) => {
    if (!isRecord(value)) throw new Error('필수 매체 정의가 올바르지 않습니다.');
    const key = requireString(value, 'key');
    if (keys.has(key)) throw new Error(`필수 매체 키가 중복됩니다: ${key}`);
    keys.add(key);
    const outputFilename = requireString(value, 'output_filename');
    if (outputFilename.includes('/') || outputFilename.includes('\\')) {
      throw new Error('출력 파일명에 경로를 넣을 수 없습니다.');
    }
    const sourceSha256 = requireHash(value, 'source_sha256');
    const targetSha256 = requireHash(value, 'target_sha256');
    return {
      key,
      label: requireString(value, 'label'),
      outputFilename,
      sourceSize: requireSize(value, 'source_size'),
      sourceSha256,
      targetSize: requireSize(value, 'target_size'),
      targetSha256,
    };
  });
  return {
    kind: parsed.kind,
    id: requireString(parsed, 'id'),
    title: requireString(parsed, 'title'),
    members,
  };
}

export function readPatchArtifactInputMatch(json: string): PatchArtifactInputMatch {
  const parsed = parseJson(json, '입력 판정');
  if (!isRecord(parsed)) throw new Error('입력 판정 결과가 올바르지 않습니다.');
  if (parsed.kind === 'unsupported') return { kind: 'unsupported' };
  if (parsed.kind !== 'source' && parsed.kind !== 'target') {
    throw new Error('알 수 없는 입력 판정 결과입니다.');
  }
  return {
    kind: parsed.kind,
    memberKey: requireString(parsed, 'member_key'),
  };
}
