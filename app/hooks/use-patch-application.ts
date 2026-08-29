'use client';

import { useEffect, useRef, useState } from 'react';

import type {
  AssignedInput,
  RejectedInput,
} from '../components/source-member-list';
import {
  classifyPatchInput,
  materializePatchMember,
  maximumPatchArtifactBytes,
  readPatchArtifactDefinition as readDefinitionFromCore,
} from '../lib/patch-core';
import {
  type PatchArtifactDefinition,
  readPatchArtifactDefinition,
  readPatchArtifactInputMatch,
} from '../lib/patch-definition';

type RunState =
  | { kind: 'idle' }
  | { kind: 'working'; message: string }
  | { kind: 'success'; message: string }
  | { kind: 'error'; message: string };

export type PatchDownload = {
  filename: string;
  key: string;
  label: string;
  size: number;
  url: string;
};

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}

function errorMessage(error: unknown) {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return '알 수 없는 오류가 발생했습니다.';
}

export function usePatchApplication() {
  const [artifactFile, setArtifactFile] = useState<File | null>(null);
  const [artifactBytes, setArtifactBytes] = useState<Uint8Array | null>(null);
  const [definition, setDefinition] = useState<PatchArtifactDefinition | null>(null);
  const [artifactError, setArtifactError] = useState<string | null>(null);
  const [assignments, setAssignments] = useState<Record<string, AssignedInput>>({});
  const [rejected, setRejected] = useState<RejectedInput[]>([]);
  const [run, setRun] = useState<RunState>({ kind: 'idle' });
  const [downloads, setDownloads] = useState<PatchDownload[]>([]);
  const artifactReadSequence = useRef(0);
  const inputReadSequence = useRef(0);
  const nextRejectedId = useRef(1);

  useEffect(() => {
    return () => downloads.forEach((download) => URL.revokeObjectURL(download.url));
  }, [downloads]);

  const assignedCount = Object.keys(assignments).length;
  const hasDefinition = artifactBytes !== null && definition !== null;
  const isReady = definition !== null && assignedCount === definition.members.length;
  const canRun = isReady && artifactBytes !== null && run.kind !== 'working';
  const status = (() => {
    if (run.kind !== 'idle') return run;
    if (artifactError) return { kind: 'error' as const, message: artifactError };
    if (!hasDefinition) return { kind: 'idle' as const, message: '패치 ZIP을 선택하세요.' };
    if (assignedCount === 0) {
      return { kind: 'idle' as const, message: '지원 원본 HDM을 한 장 이상 선택하세요.' };
    }
    if (!isReady) {
      const missingLabels = definition!.members
        .filter((member) => assignments[member.key] === undefined)
        .map((member) => member.label);
      return {
        kind: 'idle' as const,
        message: `필수 매체 ${assignedCount}/${definition!.members.length}개가 확인됐습니다. 누락: ${missingLabels.join(', ')}`,
      };
    }
    return {
      kind: 'idle' as const,
      message: `필수 매체 ${assignedCount}개의 SHA-256이 모두 확인됐습니다.`,
    };
  })();

  function resetResult() {
    setRun({ kind: 'idle' });
    setDownloads([]);
  }

  function clearInputs() {
    inputReadSequence.current += 1;
    setAssignments({});
    setRejected([]);
    resetResult();
  }

  async function selectArtifact(file: File | null) {
    const sequence = ++artifactReadSequence.current;
    inputReadSequence.current += 1;
    resetResult();
    setAssignments({});
    setRejected([]);
    setArtifactFile(file);
    setArtifactBytes(null);
    setDefinition(null);
    setArtifactError(null);
    if (!file) return;
    try {
      const maximumBytes = await maximumPatchArtifactBytes();
      if (file.size > maximumBytes) {
        throw new Error(`패치 ZIP이 너무 큽니다. 최대 ${formatBytes(maximumBytes)}까지 지원합니다.`);
      }
      const bytes = new Uint8Array(await file.arrayBuffer());
      const parsed = readPatchArtifactDefinition(await readDefinitionFromCore(bytes));
      if (artifactReadSequence.current === sequence) {
        setArtifactBytes(bytes);
        setDefinition(parsed);
      }
    } catch (error: unknown) {
      if (artifactReadSequence.current === sequence) setArtifactError(errorMessage(error));
    }
  }

  async function addInputFiles(files: File[]) {
    if (!artifactBytes || !definition || run.kind === 'working') return;
    const sequence = ++inputReadSequence.current;
    resetResult();
    setRun({ kind: 'working', message: `${files.length}개 입력의 크기와 SHA-256을 확인하고 있습니다.` });
    const nextAssignments = { ...assignments };
    const nextRejected = [...rejected];

    for (const file of files) {
      if (inputReadSequence.current !== sequence) return;
      const hasCandidateSize = definition.members.some(
        (member) => file.size === member.sourceSize || file.size === member.targetSize,
      );
      if (!hasCandidateSize) {
        nextRejected.push({
          id: nextRejectedId.current++,
          file,
          reason: '어느 필수 매체의 선언 크기와도 다릅니다.',
        });
        continue;
      }
      try {
        const inputBytes = new Uint8Array(await file.arrayBuffer());
        const matched = readPatchArtifactInputMatch(
          await classifyPatchInput(inputBytes, artifactBytes),
        );
        if (matched.kind === 'unsupported') {
          nextRejected.push({
            id: nextRejectedId.current++,
            file,
            reason: '크기는 후보와 같지만 SHA-256이 일치하지 않습니다.',
          });
          continue;
        }
        const member = definition.members.find((candidate) => candidate.key === matched.memberKey);
        if (!member) throw new Error('코어가 선언되지 않은 필수 매체를 반환했습니다.');
        if (nextAssignments[matched.memberKey]) {
          nextRejected.push({
            id: nextRejectedId.current++,
            file,
            reason: `${member.label}에 이미 정확한 입력이 있어 중복으로 분류했습니다.`,
          });
          continue;
        }
        nextAssignments[matched.memberKey] = { file, kind: matched.kind };
      } catch (error: unknown) {
        nextRejected.push({
          id: nextRejectedId.current++,
          file,
          reason: `파일을 검사하지 못했습니다: ${errorMessage(error)}`,
        });
      }
    }

    if (inputReadSequence.current !== sequence) return;
    setAssignments(nextAssignments);
    setRejected(nextRejected);
    setRun({ kind: 'idle' });
  }

  function removeMember(key: string) {
    inputReadSequence.current += 1;
    setAssignments((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
    resetResult();
  }

  function removeRejected(id: number) {
    inputReadSequence.current += 1;
    setRejected((current) => current.filter((input) => input.id !== id));
    resetResult();
  }

  async function createPatchedImages() {
    if (!artifactBytes || !definition || !isReady) return;
    resetResult();
    setRun({ kind: 'working', message: '모든 필수 매체를 검사하고 적용하고 있습니다.' });
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    try {
      const completed: Array<{
        filename: string;
        key: string;
        label: string;
        bytes: Uint8Array;
      }> = [];
      for (const member of definition.members) {
        const assigned = assignments[member.key];
        if (!assigned) throw new Error(`필수 매체가 누락됐습니다: ${member.label}`);
        const input = new Uint8Array(await assigned.file.arrayBuffer());
        const output = await materializePatchMember(input, artifactBytes, member.key);
        completed.push({
          filename: member.outputFilename,
          label: member.label,
          key: member.key,
          bytes: new Uint8Array(output),
        });
      }
      const nextDownloads = completed.map((output) => {
        const blob = new Blob([Uint8Array.from(output.bytes).buffer], {
          type: 'application/octet-stream',
        });
        return {
          filename: output.filename,
          key: output.key,
          label: output.label,
          size: blob.size,
          url: URL.createObjectURL(blob),
        };
      });
      setDownloads(nextDownloads);
      setRun({
        kind: 'success',
        message: `모든 ${nextDownloads.length}개 결과의 SHA-256이 일치했습니다.`,
      });
    } catch (error: unknown) {
      setRun({ kind: 'error', message: errorMessage(error) });
    }
  }

  return {
    addInputFiles,
    artifactError,
    artifactFile,
    assignments,
    canRun,
    clearInputs,
    createPatchedImages,
    definition,
    downloads,
    hasDefinition,
    isWorking: run.kind === 'working',
    rejected,
    removeMember,
    removeRejected,
    selectArtifact,
    status,
  };
}
