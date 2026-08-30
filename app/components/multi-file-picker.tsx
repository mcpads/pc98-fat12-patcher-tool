'use client';

import { useId, useRef, useState } from 'react';

type MultiFilePickerProps = {
  disabled?: boolean;
  onFiles: (files: File[]) => void;
};

export function MultiFilePicker({ disabled = false, onFiles }: MultiFilePickerProps) {
  const inputId = useId();
  const hintId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const dragDepth = useRef(0);
  const [isDragging, setIsDragging] = useState(false);
  const [dropError, setDropError] = useState<string | null>(null);

  function resetDragState() {
    dragDepth.current = 0;
    setIsDragging(false);
  }

  function chooseFiles(list: FileList | null) {
    const files = list ? Array.from(list) : [];
    if (files.length === 0) {
      setDropError('원본 이미지 파일을 하나 이상 선택하세요.');
      return;
    }
    setDropError(null);
    onFiles(files);
  }

  function openPicker() {
    if (disabled || !inputRef.current) return;
    inputRef.current.value = '';
    inputRef.current.click();
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
      <label className="visually-hidden" htmlFor={inputId}>원본 이미지 선택</label>
      <button
        className="file-drop"
        type="button"
        disabled={disabled}
        aria-describedby={hintId}
        onClick={openPicker}
      >
        <span className="file-badge" aria-hidden="true">IMG</span>
        <span className="file-copy">
          <strong>{isDragging ? '여기에 놓으세요' : '원본 이미지 선택'}</strong>
          <small id={hintId}>
            여러 장을 한 번에 놓아도 됩니다. 이름과 순서가 아니라 SHA-256으로 대응합니다.
          </small>
        </span>
      </button>
      <input
        ref={inputRef}
        id={inputId}
        className="visually-hidden"
        type="file"
        accept=".hdm,.img,.iso,.bin,application/octet-stream"
        disabled={disabled}
        multiple
        onChange={(event) => chooseFiles(event.target.files)}
      />
      {dropError && <p className="file-error" role="alert">{dropError}</p>}
    </div>
  );
}
