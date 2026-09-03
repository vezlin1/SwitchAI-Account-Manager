import { useEffect, useRef, useState } from 'react'
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  ExternalLink,
  Loader2,
  RefreshCw,
  Sparkles,
  X
} from 'lucide-react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { UpdateCheckResult, UpdateProgress } from '../../types'
import { api, describeIpcError } from '../../api'
import { useDialogFocus } from '../../hooks/useDialogFocus'
import { usePlatform } from '../../hooks/usePlatform'

type UpdateModalProps = {
  isOpen: boolean
  onClose: () => void
  updateInfo: UpdateCheckResult | null
  onDismissVersion?: (version: string) => Promise<void>
}

function SimpleMarkdown({ content }: { content: string }) {
  if (!content.trim()) {
    return <p className="text-xs text-ag-muted italic">No release notes available.</p>
  }

  const lines = content.split('\n')
  return (
    <div className="flex flex-col gap-1 text-xs text-ag-muted leading-relaxed font-sans select-text">
      {lines.map((line, idx) => {
        const trimmed = line.trim()
        if (!trimmed) {
          return <div key={idx} className="h-1.5" />
        }

        if (trimmed === '---' || trimmed === '***') {
          return <hr key={idx} className="border-white/[0.08] my-2" />
        }

        // Heading 1-3
        if (trimmed.startsWith('### ')) {
          return (
            <h4 key={idx} className="text-xs font-semibold text-white mt-2 mb-0.5">
              {trimmed.slice(4)}
            </h4>
          )
        }
        if (trimmed.startsWith('## ')) {
          return (
            <h3 key={idx} className="text-sm font-semibold text-white mt-2.5 mb-1">
              {trimmed.slice(3)}
            </h3>
          )
        }
        if (trimmed.startsWith('# ')) {
          return (
            <h2 key={idx} className="text-sm font-bold text-white mt-3 mb-1">
              {trimmed.slice(2)}
            </h2>
          )
        }

        // Bullet point
        const isBullet = trimmed.startsWith('- ') || trimmed.startsWith('* ')
        const textContent = isBullet ? trimmed.slice(2) : trimmed

        // Parse inline markdown: [link](url), **bold**, `code`
        const parts = textContent.split(/(\[.*?\]\(.*?\)|\*\*.*?\*\*|`.*?`)/g)
        const renderedParts = parts.map((part, pIdx) => {
          if (part.startsWith('[') && part.includes('](') && part.endsWith(')')) {
            const match = part.match(/^\[(.*?)\]\((.*?)\)$/)
            if (match) {
              const [, label, url] = match
              return (
                <button
                  key={pIdx}
                  type="button"
                  onClick={() => void api.openExternalUrl(url)}
                  className="text-blue-400 hover:text-blue-300 underline font-medium cursor-pointer inline"
                >
                  {label}
                </button>
              )
            }
          }
          if (part.startsWith('**') && part.endsWith('**')) {
            return (
              <strong key={pIdx} className="font-semibold text-ag-text">
                {part.slice(2, -2)}
              </strong>
            )
          }
          if (part.startsWith('`') && part.endsWith('`')) {
            return (
              <code
                key={pIdx}
                className="px-1 py-0.5 rounded bg-white/[0.07] text-[11px] font-mono text-ag-text"
              >
                {part.slice(1, -1)}
              </code>
            )
          }
          return part
        })

        if (isBullet) {
          return (
            <div key={idx} className="flex items-start gap-2 pl-1">
              <span className="text-blue-400 select-none mt-0.5">•</span>
              <span className="flex-1">{renderedParts}</span>
            </div>
          )
        }

        return <p key={idx}>{renderedParts}</p>
      })}
    </div>
  )
}

export function UpdateModal({
  isOpen,
  onClose,
  updateInfo,
  onDismissVersion
}: UpdateModalProps) {
  const { isMac } = usePlatform()
  const [stage, setStage] = useState<'info' | 'downloading' | 'ready' | 'error'>('info')
  const [progress, setProgress] = useState<UpdateProgress | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const dialogRef = useRef<HTMLDivElement>(null)

  const handleClose = () => {
    setStage('info')
    setProgress(null)
    setErrorMsg(null)
    setBusy(false)
    onClose()
  }

  useDialogFocus(dialogRef, handleClose, !busy && stage !== 'downloading')

  // Listen for download progress events from Tauri backend
  useEffect(() => {
    if (!isOpen) return
    let unlisten: UnlistenFn | undefined

    void listen<UpdateProgress>('update://progress', (event) => {
      setProgress(event.payload)
    }).then((fn) => {
      unlisten = fn
    })

    return () => {
      unlisten?.()
    }
  }, [isOpen])

  if (!isOpen || !updateInfo) return null

  const handleStartDownload = async () => {
    try {
      setStage('downloading')
      setErrorMsg(null)
      setBusy(true)
      await api.downloadAndStageUpdate()
      setStage('ready')
    } catch (err: unknown) {
      setStage('error')
      setErrorMsg(describeIpcError(err))
    } finally {
      setBusy(false)
    }
  }

  const handleRestartNow = async () => {
    try {
      setBusy(true)
      await api.installUpdateAndRestart()
    } catch (err: unknown) {
      setStage('error')
      setErrorMsg(describeIpcError(err))
      setBusy(false)
    }
  }

  const handleSkipVersion = async () => {
    try {
      setBusy(true)
      if (onDismissVersion) {
        await onDismissVersion(updateInfo.version)
      } else {
        await api.dismissUpdateVersion(updateInfo.version)
      }
      onClose()
    } catch (err) {
      console.error('Failed to dismiss version:', err)
      onClose()
    } finally {
      setBusy(false)
    }
  }

  const formatBytes = (bytes?: number | null) => {
    if (!bytes || bytes <= 0) return null
    const mb = bytes / (1024 * 1024)
    return `${mb.toFixed(1)} MB`
  }

  return (
    <div className="settings-backdrop" role="presentation">
      <div
        ref={dialogRef}
        className="settings-dialog max-w-lg w-full"
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-dialog-title"
        tabIndex={-1}
      >
        {/* Header */}
        <div className="settings-dialog-header">
          <div id="update-dialog-title" className="settings-dialog-title flex items-center gap-2">
            <Sparkles size={16} className="text-blue-400" />
            <span>SwitchAI Update</span>
          </div>
          <button
            type="button"
            className="settings-dialog-close"
            onClick={handleClose}
            disabled={busy || stage === 'downloading'}
            title="Close"
            aria-label="Close"
          >
            <X size={15} />
          </button>
        </div>

        {/* Body */}
        <div className="settings-dialog-body flex flex-col gap-4 py-3">
          {stage === 'info' && (
            <>
              {isMac && (
                <div className="flex items-start gap-2.5 p-3 rounded-xl bg-amber-500/10 border border-amber-500/20 text-xs text-amber-300">
                  <AlertTriangle size={16} className="shrink-0 mt-0.5 text-amber-400" />
                  <div className="flex flex-col gap-0.5">
                    <span className="font-semibold text-white">Manual update recommended for macOS</span>
                    <span className="text-ag-muted leading-relaxed">
                      To preserve application code signature integrity, in-place updates are disabled on macOS. Please download the DMG package from GitHub Releases.
                    </span>
                  </div>
                </div>
              )}

              <div className="flex items-center justify-between p-3 rounded-xl bg-white/[0.03] border border-white/[0.06]">
                <div className="flex flex-col">
                  <span className="text-xs text-ag-muted">Update Available</span>
                  <div className="flex items-center gap-2 mt-0.5">
                    <span className="text-base font-semibold text-white">v{updateInfo.version}</span>
                    <span className="text-xs px-2 py-0.5 rounded-full bg-blue-500/15 text-blue-400 border border-blue-500/25">
                      New
                    </span>
                  </div>
                </div>
                <div className="text-right">
                  <span className="text-[11px] text-ag-muted block">Current version</span>
                  <span className="text-xs font-medium text-ag-text">v{updateInfo.currentVersion}</span>
                </div>
              </div>

              {updateInfo.downloadSize && (
                <div className="text-xs text-ag-muted flex items-center gap-2">
                  <span>Download size:</span>
                  <span className="font-medium text-ag-text">{formatBytes(updateInfo.downloadSize)}</span>
                </div>
              )}

              {/* Release Notes */}
              <div className="flex flex-col gap-1.5">
                <span className="text-xs font-medium text-ag-text">What's New:</span>
                <div className="max-h-56 overflow-y-auto p-3 rounded-lg bg-black/30 border border-white/[0.06] text-xs text-ag-muted custom-scrollbar">
                  <SimpleMarkdown content={updateInfo.releaseNotes || ''} />
                </div>
              </div>
            </>
          )}

          {stage === 'downloading' && (
            <div className="py-6 flex flex-col items-center justify-center gap-4 text-center">
              <div className="w-12 h-12 rounded-2xl bg-blue-500/10 border border-blue-500/20 flex items-center justify-center text-blue-400 animate-pulse">
                <Download size={24} />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-white">Downloading update...</h3>
                <p className="text-xs text-ag-muted mt-1">
                  Downloading and cryptographically verifying package
                </p>
              </div>

              {/* Progress Bar */}
              <div className="w-full max-w-xs flex flex-col gap-1.5 mt-2">
                <div className="w-full h-2 rounded-full bg-white/[0.06] overflow-hidden">
                  <div
                    className="h-full bg-blue-500 transition-all duration-150 rounded-full"
                    style={{ width: `${Math.max(5, Math.min(100, progress?.percent ?? 0))}%` }}
                  />
                </div>
                <div className="flex justify-between text-[11px] text-ag-muted">
                  <span>
                    {progress ? `${progress.percent.toFixed(0)}%` : 'Preparing...'}
                  </span>
                  {progress && progress.totalBytes && (
                    <span>
                      {formatBytes(progress.downloadedBytes)} / {formatBytes(progress.totalBytes)}
                    </span>
                  )}
                </div>
              </div>
            </div>
          )}

          {stage === 'ready' && (
            <div className="py-5 flex flex-col items-center justify-center gap-3 text-center">
              <div className="w-12 h-12 rounded-2xl bg-emerald-500/10 border border-emerald-500/20 flex items-center justify-center text-emerald-400">
                <CheckCircle2 size={24} />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-white">Update ready to install!</h3>
                <p className="text-xs text-ag-muted mt-1 max-w-sm">
                  The update has been downloaded and verified. Click "Restart Now" to apply the update.
                </p>
              </div>
            </div>
          )}

          {stage === 'error' && (
            <div className="py-4 flex flex-col items-center justify-center gap-3 text-center">
              <div className="w-12 h-12 rounded-2xl bg-rose-500/10 border border-rose-500/20 flex items-center justify-center text-rose-400">
                <AlertTriangle size={24} />
              </div>
              <div>
                <h3 className="text-sm font-semibold text-white">Update failed</h3>
                <p className="text-xs text-rose-400 mt-1 max-w-sm text-center">
                  {errorMsg || 'Failed to download or apply the update.'}
                </p>
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="settings-dialog-footer flex items-center justify-between gap-2">
          {stage === 'info' && (
            <>
              <button
                type="button"
                className="text-xs text-ag-muted hover:text-white transition-colors cursor-pointer select-none"
                onClick={() => void handleSkipVersion()}
                disabled={busy}
              >
                Skip this version
              </button>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  className="settings-cancel"
                  onClick={handleClose}
                  disabled={busy}
                >
                  Later
                </button>
                {isMac ? (
                  <button
                    type="button"
                    className="settings-save inline-flex items-center gap-1.5"
                    onClick={() => {
                      void api.openExternalUrl(
                        'https://github.com/vezlin1/SwitchAI-Account-Manager/releases/latest'
                      )
                      handleClose()
                    }}
                  >
                    <ExternalLink size={14} />
                    <span>Download DMG</span>
                  </button>
                ) : (
                  <button
                    type="button"
                    className="settings-save inline-flex items-center gap-1.5"
                    onClick={() => void handleStartDownload()}
                    disabled={busy}
                  >
                    <Download size={14} />
                    <span>Update Now</span>
                  </button>
                )}
              </div>
            </>
          )}

          {stage === 'downloading' && (
            <div className="w-full flex justify-end">
              <span className="text-xs text-ag-muted flex items-center gap-2">
                <Loader2 size={13} className="animate-spin text-blue-400" />
                Please keep the app open...
              </span>
            </div>
          )}

          {stage === 'ready' && (
            <div className="w-full flex items-center justify-between">
              <button
                type="button"
                className="settings-cancel"
                onClick={handleClose}
                disabled={busy}
              >
                Restart later
              </button>
              <button
                type="button"
                className="settings-save !bg-emerald-600 hover:!bg-emerald-500 inline-flex items-center gap-1.5"
                onClick={() => void handleRestartNow()}
                disabled={busy}
              >
                {busy ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <RefreshCw size={14} />
                )}
                <span>Restart Now</span>
              </button>
            </div>
          )}

          {stage === 'error' && (
            <div className="w-full flex items-center justify-between">
              <button
                type="button"
                className="settings-cancel"
                onClick={handleClose}
              >
                Close
              </button>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  className="h-8 px-3 rounded-lg border border-white/[0.1] bg-white/[0.04] text-xs font-medium text-ag-text hover:bg-white/[0.09] hover:text-white inline-flex items-center gap-1.5 transition-all cursor-pointer"
                  onClick={() => api.openExternalUrl('https://github.com/vezlin1/SwitchAI-Account-Manager/releases/latest')}
                >
                  <ExternalLink size={13} />
                  <span>GitHub Releases</span>
                </button>
                {!isMac && (
                  <button
                    type="button"
                    className="settings-save inline-flex items-center gap-1.5"
                    onClick={() => void handleStartDownload()}
                  >
                    <RefreshCw size={14} />
                    <span>Retry</span>
                  </button>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
