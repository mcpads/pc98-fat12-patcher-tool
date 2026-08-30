import type { PatchArtifactMember } from '../lib/patch-definition';

export type AssignedInput = {
  file: File;
  kind: 'source' | 'target';
};

export type RejectedInput = {
  id: number;
  file: File;
  reason: string;
};

type SourceMemberListProps = {
  assignments: Record<string, AssignedInput>;
  disabled?: boolean;
  members: PatchArtifactMember[];
  rejected: RejectedInput[];
  onClearAll: () => void;
  onRemoveMember: (key: string) => void;
  onRemoveRejected: (id: number) => void;
};

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}

export function SourceMemberList({
  assignments,
  disabled = false,
  members,
  rejected,
  onClearAll,
  onRemoveMember,
  onRemoveRejected,
}: SourceMemberListProps) {
  const assignedCount = Object.keys(assignments).length;

  return (
    <div className="source-summary">
      <div className="source-summary-heading">
        <strong>필수 매체 {assignedCount}/{members.length}</strong>
        {(assignedCount > 0 || rejected.length > 0) && (
          <button type="button" disabled={disabled} onClick={onClearAll}>입력 모두 지우기</button>
        )}
      </div>
      <ul className="member-list">
        {members.map((member) => {
          const assigned = assignments[member.key];
          return (
            <li className={assigned ? 'is-matched' : 'is-missing'} key={member.key}>
              <div className="member-copy">
                <strong>{member.label}</strong>
                {assigned ? (
                  <small>
                    {assigned.kind === 'source' ? '적용 예정' : '이미 적용됨'} · {assigned.file.name} · {formatBytes(assigned.file.size)}
                  </small>
                ) : (
                  <small>필요한 원본 또는 정확한 적용 결과가 없습니다.</small>
                )}
                <dl className="member-identity">
                  <div>
                    <dt>지원 원본 · {formatBytes(member.sourceSize)}</dt>
                    <dd><span>SHA-256</span><code>{member.sourceSha256}</code></dd>
                  </div>
                  <div>
                    <dt>적용 결과 · {formatBytes(member.targetSize)}</dt>
                    <dd><span>SHA-256</span><code>{member.targetSha256}</code></dd>
                  </div>
                </dl>
              </div>
              {assigned && (
                <button type="button" disabled={disabled} onClick={() => onRemoveMember(member.key)}>지우기</button>
              )}
            </li>
          );
        })}
      </ul>
      {rejected.length > 0 && (
        <div className="rejected-inputs">
          <strong>대응하지 않은 입력</strong>
          <ul>
            {rejected.map((input) => (
              <li key={input.id}>
                <div><span>{input.file.name}</span><small>{input.reason}</small></div>
                <button type="button" disabled={disabled} onClick={() => onRemoveRejected(input.id)}>지우기</button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
