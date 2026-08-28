'use client';

import { useEffect, useMemo, useRef, useState } from 'react';

import { FilePicker } from './components/file-picker';
import {
  makePatchedImage,
  maximumPatchPackageBytes,
  readPatchPackageRecipe,
} from './lib/patch-core';
import {
  type RecipeSummary,
  readRecipeSummary,
} from './lib/patch-definition';

type RunState =
  | { kind: 'idle' }
  | { kind: 'working'; message: string }
  | { kind: 'success'; message: string }
  | { kind: 'error'; message: string };

type Download = {
  filename: string;
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

export default function Home() {
  const [source, setSource] = useState<File | null>(null);
  const [packageFile, setPackageFile] = useState<File | null>(null);
  const [packageBytes, setPackageBytes] = useState<Uint8Array | null>(null);
  const [packageSummary, setPackageSummary] = useState<RecipeSummary | null>(null);
  const [packageError, setPackageError] = useState<string | null>(null);
  const [run, setRun] = useState<RunState>({ kind: 'idle' });
  const [download, setDownload] = useState<Download | null>(null);
  const packageReadSequence = useRef(0);

  useEffect(() => {
    return () => {
      if (download) URL.revokeObjectURL(download.url);
    };
  }, [download]);

  const activeSummary = packageSummary;
  const hasDefinition = packageBytes !== null && packageSummary !== null;
  const sourceSizeMatches = source === null || activeSummary === null || source.size === activeSummary.sourceSize;
  const canRun = source !== null
    && hasDefinition
    && sourceSizeMatches
    && run.kind !== 'working';
  const status = useMemo(() => {
    if (run.kind !== 'idle') return run;
    if (packageError) return { kind: 'error', message: packageError };
    if (!hasDefinition) return { kind: 'idle', message: '패치 ZIP을 선택하세요.' };
    if (!source) return { kind: 'idle', message: '지원 원본 HDM을 선택하세요.' };
    if (!sourceSizeMatches) {
      return {
        kind: 'error',
        message: `원본 크기가 다릅니다. ${formatBytes(activeSummary!.sourceSize)} 파일이 필요합니다.`,
      };
    }
    return { kind: 'idle', message: '입력이 준비됐습니다. 원본 검사부터 시작합니다.' };
  }, [activeSummary, hasDefinition, packageError, run, source, sourceSizeMatches]);

  function resetResult() {
    setRun({ kind: 'idle' });
    setDownload((current) => {
      if (current) URL.revokeObjectURL(current.url);
      return null;
    });
  }

  async function selectPackage(file: File | null) {
    const sequence = ++packageReadSequence.current;
    resetResult();
    setPackageFile(file);
    setPackageBytes(null);
    setPackageSummary(null);
    setPackageError(null);
    if (!file) return;
    try {
      const maximumBytes = await maximumPatchPackageBytes();
      if (file.size > maximumBytes) {
        throw new Error(`패치 ZIP이 너무 큽니다. 최대 ${formatBytes(maximumBytes)}까지 지원합니다.`);
      }
      const bytes = new Uint8Array(await file.arrayBuffer());
      const summary = readRecipeSummary(await readPatchPackageRecipe(bytes));
      if (packageReadSequence.current === sequence) {
        setPackageBytes(bytes);
        setPackageSummary(summary);
      }
    } catch (error: unknown) {
      if (packageReadSequence.current === sequence) setPackageError(errorMessage(error));
    }
  }

  async function createPatchedImage() {
    if (!source || !packageBytes || !activeSummary) return;
    resetResult();
    setRun({ kind: 'working', message: '원본 해시와 FAT12 구조를 확인하고 있습니다.' });
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    try {
      const sourceBytes = new Uint8Array(await source.arrayBuffer());
      if (sourceBytes.length !== activeSummary.sourceSize) {
        throw new Error(`원본 크기가 다릅니다. ${activeSummary.sourceSize}바이트 파일이 필요합니다.`);
      }
      setRun({ kind: 'working', message: '파일별 패치를 적용하고 결과 HDM을 조립하고 있습니다.' });
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      const output = await makePatchedImage(sourceBytes, packageBytes);
      const blob = new Blob([new Uint8Array(output)], { type: 'application/octet-stream' });
      const nextDownload = {
        filename: activeSummary.outputFilename,
        size: blob.size,
        url: URL.createObjectURL(blob),
      };
      setDownload(nextDownload);
      setRun({
        kind: 'success',
        message: '출력 SHA-256까지 일치했습니다. 결과 HDM을 내려받으세요.',
      });
    } catch (error: unknown) {
      setRun({ kind: 'error', message: errorMessage(error) });
    }
  }

  return (
    <main className="site-shell">
      <header className="app-header">
        <div>
          <p>RetroGame Patcher / PC-98 FAT12</p>
          <h1>한글패치 적용</h1>
          <span>패치 ZIP을 먼저 확인한 다음, 지원되는 원본 HDM에 적용합니다.</span>
        </div>
        <strong className="privacy-note">파일 전송 없음 · 브라우저 내부 처리</strong>
      </header>

      <section className="patch-card" aria-labelledby="patch-title">
        <div className="card-heading">
          <div>
            <h2 id="patch-title">{activeSummary?.title ?? '패치 적용하기'}</h2>
            <p>두 파일을 차례로 끌어 놓거나 눌러서 선택하세요.</p>
          </div>
        </div>

        <div className="input-section definition-section">
          <div className="section-heading">
            <span>01</span>
            <div><strong>패치 ZIP</strong><small>먼저 recipe.json과 파일별 BPS를 검사합니다.</small></div>
          </div>
          <div className="definition-file">
            <FilePicker
              accept=".zip,application/zip,application/x-zip-compressed"
              badge="ZIP"
              file={packageFile}
              hint="패치 ZIP을 여기에 놓거나 눌러서 선택"
              label="패치 ZIP 선택"
              validation={packageError ? 'invalid' : (packageSummary ? 'valid' : 'unchecked')}
              onChange={(file) => {
                setSource(null);
                void selectPackage(file);
              }}
            />
          </div>
          {activeSummary && (
            <dl className="identity-row">
              <div><dt>원본</dt><dd>{activeSummary.sourceSha256.slice(0, 12)}…</dd></div>
              <div><dt>결과</dt><dd>{activeSummary.targetSha256.slice(0, 12)}…</dd></div>
              <div><dt>크기</dt><dd>{formatBytes(activeSummary.sourceSize)}</dd></div>
            </dl>
          )}
        </div>

        <div className="input-section">
          <div className="section-heading">
            <span>02</span>
            <div><strong>원본 HDM</strong><small>패치가 지원하는 원본을 선택합니다.</small></div>
          </div>
          <FilePicker
            accept=".hdm,application/octet-stream"
            badge="HDM"
            disabled={!hasDefinition}
            file={source}
            hint={hasDefinition
              ? '원본 HDM을 여기에 놓거나 눌러서 선택'
              : '먼저 패치 ZIP을 선택하세요.'}
            label="HDM 파일을 선택하세요"
            validation={source && !sourceSizeMatches
              ? 'invalid'
              : (source && run.kind === 'success' ? 'valid' : 'unchecked')}
            onChange={(file) => { resetResult(); setSource(file); }}
          />
        </div>

        <div className={`action-row status-${status.kind}`} aria-live="polite">
          <p><span aria-hidden="true">●</span>{status.message}</p>
          {download ? (
            <a className="primary-action download-action" href={download.url} download={download.filename}>
              HDM 내려받기 <small>{formatBytes(download.size)}</small>
            </a>
          ) : (
            <button className="primary-action" type="button" disabled={!canRun} onClick={createPatchedImage}>
              {run.kind === 'working' ? '처리 중…' : '검사하고 적용하기'} <span aria-hidden="true">→</span>
            </button>
          )}
        </div>
      </section>

      <footer>
        <p>원본과 결과는 이 탭의 메모리에서만 처리됩니다.</p>
        <a
          href="https://github.com/mcpads/pc98-fat12-patcher-tool"
          target="_blank"
          rel="noopener noreferrer"
        >
          소스 코드 (GitHub)
        </a>
      </footer>
    </main>
  );
}
