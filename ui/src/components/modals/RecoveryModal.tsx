import { useRef, useState } from 'react'
import { FolderOpen, Loader2, RotateCcw, Sparkles } from 'lucide-react'
import type { RecoveryStatus } from '../../types'
import { useDialogFocus } from '../../hooks/useDialogFocus'

type RecoveryModalProps = {
  recovery: RecoveryStatus
  loading: boolean
  onRestore: () => Promise<unknown>
  onStartFresh: () => Promise<unknown>
  onOpenDataDirectory: () => Promise<unknown>
}

export function RecoveryModal({
  recovery,
  loading,
  onRestore,
  onStartFresh,
  onOpenDataDirectory
}: RecoveryModalProps) {
  const [busy, setBusy] = useState<'restore' | 'fresh' | 'open' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  useDialogFocus(dialogRef, () => undefined, false)

  const run = async (action: 'restore' | 'fresh' | 'open', handler: () => Promise<unknown>) => {
    setBusy(action)
    setError(null)
    try {
      await handler()
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="recovery-backdrop" role="presentation">
      <div
        ref={dialogRef}
        className="recovery-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="recovery-title"
        aria-describedby="recovery-message"
        aria-busy={busy != null || loading}
        tabIndex={-1}
      >
        <div className="recovery-dialog-header">
          <h2 id="recovery-title">Accounts could not be loaded</h2>
        </div>
        <div className="recovery-dialog-body">
          <p id="recovery-message">{recovery.error}</p>
          <dl className="recovery-details">
            <dt>State file</dt>
            <dd className="allow-select">{recovery.statePath}</dd>
            <dt>Data directory</dt>
            <dd className="allow-select">{recovery.dataDirectory}</dd>
            <dt>Backup available</dt>
            <dd>{recovery.backupAvailable ? 'Yes' : 'No'}</dd>
          </dl>
          <p className="recovery-hint">
            The existing state files were preserved. Restore the most recent backup, or start fresh,
            which quarantines those files and clears this app's protected token vault without changing
            Codex auth.json. You can also open the data directory to inspect or copy the files manually.
          </p>
        </div>
        <div className="recovery-dialog-actions">
          <button
            type="button"
            className="recovery-secondary"
            onClick={() => void run('fresh', onStartFresh)}
            disabled={busy != null || loading}
          >
            {busy === 'fresh'
              ? <Loader2 size={15} className="animate-spin" aria-hidden="true" />
              : <Sparkles size={15} aria-hidden="true" />}
            Start fresh
          </button>
          <button
            type="button"
            className="recovery-secondary"
            onClick={() => void run('open', onOpenDataDirectory)}
            disabled={busy != null || loading}
          >
            {busy === 'open'
              ? <Loader2 size={15} className="animate-spin" aria-hidden="true" />
              : <FolderOpen size={15} aria-hidden="true" />}
            Open data directory
          </button>
          <button
            type="button"
            className="recovery-primary"
            onClick={() => void run('restore', onRestore)}
            disabled={busy != null || loading || !recovery.backupAvailable}
          >
            {busy === 'restore'
              ? <Loader2 size={15} className="animate-spin" aria-hidden="true" />
              : <RotateCcw size={15} aria-hidden="true" />}
            Restore backup
          </button>
        </div>
        {error && (
          <div className="recovery-error" role="alert">
            {error}
          </div>
        )}
      </div>
    </div>
  )
}
