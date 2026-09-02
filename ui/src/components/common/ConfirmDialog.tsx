import { useRef } from 'react'
import { Loader2 } from 'lucide-react'
import { useDialogFocus } from '../../hooks/useDialogFocus'

type ConfirmDialogProps = {
  title: string
  message: string
  confirmLabel: string
  variant?: 'primary' | 'danger'
  busy?: boolean
  onCancel: () => void
  onConfirm: () => void
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  variant = 'primary',
  busy = false,
  onCancel,
  onConfirm
}: ConfirmDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null)
  useDialogFocus(dialogRef, onCancel, !busy)

  const confirmButtonClass = variant === 'danger' ? 'confirm-danger' : 'confirm-primary'

  return (
    <div
      className="confirm-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onCancel()
      }}
    >
      <div
        ref={dialogRef}
        className="confirm-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        aria-describedby="confirm-dialog-message"
        aria-busy={busy}
        tabIndex={-1}
      >
        <div className="confirm-dialog-header">
          <h2 id="confirm-dialog-title">{title}</h2>
        </div>
        <div className="confirm-dialog-body">
          <p id="confirm-dialog-message" className="allow-select">{message}</p>
        </div>
        <div className="confirm-dialog-footer">
          <button
            type="button"
            className="confirm-cancel"
            onClick={onCancel}
            disabled={busy}
            data-autofocus
          >
            Cancel
          </button>
          <button
            type="button"
            className={confirmButtonClass}
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? (
              <span className="flex items-center gap-1.5">
                <Loader2 size={14} className="animate-spin" aria-hidden="true" />
                Working...
              </span>
            ) : (
              confirmLabel
            )}
          </button>
        </div>
      </div>
    </div>
  )
}
