'use client';

import { useEffect, useId, useRef } from 'react';
import type { ReactNode } from 'react';

type ConfirmModalProps = {
  open: boolean;
  title: string;
  description?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
  confirmDisabled?: boolean;
  cancelDisabled?: boolean;
  destructive?: boolean;
  hideCancel?: boolean;
  children?: ReactNode;
  maxWidthClassName?: string;
  zIndexClassName?: string;
};

export default function ConfirmModal({
  open,
  title,
  description,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  onConfirm,
  onCancel,
  confirmDisabled = false,
  cancelDisabled = false,
  destructive = false,
  hideCancel = false,
  children,
  maxWidthClassName = 'max-w-sm',
  zIndexClassName = 'z-50',
}: ConfirmModalProps) {
  const titleId = useId();
  const descriptionId = `${titleId}-description`;
  const dialogRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const firstAction = dialogRef.current?.querySelector<HTMLButtonElement>('button:not(:disabled)');
    firstAction?.focus();
  }, [open]);

  if (!open) return null;

  return (
    <div
      className={`fixed inset-0 ${zIndexClassName} flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]`}
      role="presentation"
      onClick={onCancel}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descriptionId : undefined}
        className={`panel rf-preserve-surface w-full ${maxWidthClassName} space-y-4 rounded-2xl border border-[var(--border)] p-6`}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="space-y-2">
          <h2 id={titleId} className="text-lg font-semibold">{title}</h2>
          {description ? <div id={descriptionId} className="text-sm muted">{description}</div> : null}
        </div>

        {children}

        <div className="flex justify-end gap-2">
          {!hideCancel ? (
            <button
              type="button"
              onClick={onCancel}
              className="btn-ghost px-4 py-2 text-sm"
              disabled={cancelDisabled}
            >
              {cancelLabel}
            </button>
          ) : null}
          <button
            type="button"
            onClick={onConfirm}
            className={`${destructive ? 'btn-danger' : 'btn-primary'} px-4 py-2 text-sm`}
            disabled={confirmDisabled}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
