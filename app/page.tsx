'use client';

import { FilePicker } from './components/file-picker';
import { MultiFilePicker } from './components/multi-file-picker';
import { SourceMemberList } from './components/source-member-list';
import { usePatchApplication } from './hooks/use-patch-application';

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}

export default function Home() {
  const patch = usePatchApplication();

  return (
    <main className="site-shell">
      <header className="app-header">
        <div>
          <p>RetroGame Patcher / PC-98 FAT12</p>
          <h1>한글패치 적용</h1>
          <span>패치 ZIP을 확인한 다음, 필요한 원본 HDM을 해시로 자동 대응합니다.</span>
        </div>
        <strong className="privacy-note">파일 전송 없음 · 브라우저 내부 처리</strong>
      </header>

      <section className="patch-card" aria-labelledby="patch-title">
        <div className="card-heading">
          <div>
            <h2 id="patch-title">{patch.definition?.title ?? '패치 적용하기'}</h2>
            <p>단일 패치와 여러 디스크 패치 세트를 같은 흐름으로 적용합니다.</p>
          </div>
        </div>

        <div className="input-section definition-section">
          <div className="section-heading">
            <span>01</span>
            <div><strong>패치 ZIP</strong><small>먼저 패키지 구조와 내포된 파일별 BPS를 검사합니다.</small></div>
          </div>
          <div className="definition-file">
            <FilePicker
              accept=".zip,application/zip,application/x-zip-compressed"
              badge="ZIP"
              disabled={patch.isWorking}
              file={patch.artifactFile}
              hint="패치 ZIP을 여기에 놓거나 눌러서 선택"
              label="패치 ZIP 선택"
              validation={patch.artifactError ? 'invalid' : (patch.definition ? 'valid' : 'unchecked')}
              onChange={(file) => { void patch.selectArtifact(file); }}
            />
          </div>
          {patch.definition && (
            <dl className="identity-row artifact-identity-row">
              <div><dt>형식</dt><dd>{patch.definition.kind === 'single' ? '단일 패키지' : '패치 세트'}</dd></div>
              <div><dt>필수 매체</dt><dd>{patch.definition.members.length}개</dd></div>
              <div><dt>판정</dt><dd>SHA-256</dd></div>
            </dl>
          )}
        </div>

        <div className="input-section">
          <div className="section-heading">
            <span>02</span>
            <div><strong>원본 HDM</strong><small>여러 장을 함께 놓거나 빠진 매체를 나중에 추가하세요.</small></div>
          </div>
          <MultiFilePicker
            disabled={!patch.hasDefinition || patch.isWorking}
            onFiles={(files) => { void patch.addInputFiles(files); }}
          />
          {patch.definition && (
            <SourceMemberList
              assignments={patch.assignments}
              disabled={patch.isWorking}
              members={patch.definition.members}
              rejected={patch.rejected}
              onClearAll={patch.clearInputs}
              onRemoveMember={patch.removeMember}
              onRemoveRejected={patch.removeRejected}
            />
          )}
        </div>

        <div className={`action-row status-${patch.status.kind}`} aria-live="polite">
          <p><span aria-hidden="true">●</span>{patch.status.message}</p>
          {patch.downloads.length > 0 ? (
            <div className="download-list">
              {patch.downloads.map((download) => (
                <a
                  className="primary-action download-action"
                  href={download.url}
                  download={download.filename}
                  key={download.key}
                >
                  <span>{patch.downloads.length === 1 ? 'HDM 내려받기' : download.label}</span>
                  <small>{formatBytes(download.size)}</small>
                </a>
              ))}
            </div>
          ) : (
            <button
              className="primary-action"
              type="button"
              disabled={!patch.canRun}
              onClick={patch.createPatchedImages}
            >
              {patch.isWorking ? '처리 중…' : '검사하고 적용하기'} <span aria-hidden="true">→</span>
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
