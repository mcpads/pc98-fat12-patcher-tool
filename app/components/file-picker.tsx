'use client';

import { useId, useRef, useState } from 'react';

type FilePickerProps = {
  accept: string;
  badge: string;
  disabled?: boolean;
  file: File | null;
  hint: string;
  label: string;
  onChange: (file: File | null) => void;
  validation?: 'unchecked' | 'valid' | 'invalid';
};

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}

export function FilePicker({
  accept,
  badge,
  disabled = false,
  file,
  hint,
  label,
  onChange,
  validation = 'unchecked',
}: FilePickerProps) {
  const inputId = useId();
  const hintId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const dragDepth = useRef(0);
  const [isDragging, setIsDragging] = useState(false);
  const [dropError, setDropError] = useState<string | null>(null);

  function openPicker() {
    if (disabled || !inputRef.current) return;
    inputRef.current.value = '';
    inputRef.current.click();
  }

  function resetDragState() {
    dragDepth.current = 0;
    setIsDragging(false);
  }

  function chooseFiles(files: FileList | null) {
    if (!files || files.length !== 1) {
      setDropError('파일 하나만 놓아주세요.');
      return;
    }
    setDropError(null);
    onChange(files[0]);
  }

  return (
    <div
      className={`picker ${isDragging ? 'is-dragging' : ''}`}
      onDragEnter={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (disabled) return;
        dragDepth.current += 1;
        setIsDragging(true);
      }}
      onDragOver={(event) => {
        event.preventDefault();
        event.stopPropagation();
        event.dataTransfer.dropEffect = disabled ? 'none' : 'copy';
      }}
      onDragLeave={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (disabled) return;
        dragDepth.current = Math.max(0, dragDepth.current - 1);
        if (dragDepth.current === 0) setIsDragging(false);
      }}
      onDragEnd={resetDragState}
      onDrop={(event) => {
        event.preventDefault();
        event.stopPropagation();
        resetDragState();
        if (!disabled) chooseFiles(event.dataTransfer.files);
      }}
    >
      <label className="visually-hidden" htmlFor={inputId}>
        {label}
      </label>
      <button
        className={`file-drop ${file ? `has-file is-${validation}` : ''}`}
        type="button"
        disabled={disabled}
        aria-describedby={hintId}
        aria-invalid={file && validation === 'invalid' ? true : undefined}
        onClick={openPicker}
      >
        <span className="file-badge" aria-hidden="true">{badge}</span>
        <span className="file-copy">
          <strong>{isDragging ? '여기에 놓으세요' : (file?.name ?? label)}</strong>
          <small id={hintId}>
            {file ? `${formatBytes(file.size)} · 다른 파일을 놓거나 눌러서 교체` : hint}
          </small>
        </span>
      </button>
      <input
        ref={inputRef}
        id={inputId}
        className="visually-hidden"
        type="file"
        accept={accept}
        disabled={disabled}
        onChange={(event) => chooseFiles(event.target.files)}
      />
      {dropError && <p className="file-error" role="alert">{dropError}</p>}
      {file && !disabled && (
        <button
          className="file-clear"
          type="button"
          onClick={() => {
            if (inputRef.current) inputRef.current.value = '';
            setDropError(null);
            onChange(null);
          }}
        >
          선택 지우기
        </button>
      )}
    </div>
  );
}
