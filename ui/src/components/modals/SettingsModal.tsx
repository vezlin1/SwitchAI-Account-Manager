import { useRef, useState } from 'react'
import { Download, Loader2, Settings, Upload, X } from 'lucide-react'
import type { Account, AppData, AppSettings, AutoRefreshStatus } from '../../types'
import { useDialogFocus } from '../../hooks/useDialogFocus'
import { Switch } from '../common/Switch'

type SettingsModalProps = {
  settings: AppSettings
  accounts?: Account[]
  status?: AutoRefreshStatus | null
  onClose: () => void
  onSave: (settings: AppSettings) => Promise<AppData | null>
  onRefreshStatus: () => Promise<AutoRefreshStatus>
  onImportAccounts?: (imported: Account[]) => Promise<void>
}

const quickIntervals = [
  { label: '15m', minutes: 15 },
  { label: '30m', minutes: 30 },
  { label: '1h', minutes: 60 },
  { label: '2h', minutes: 120 },
  { label: '4h', minutes: 240 }
]

export function SettingsModal({
  settings,
  accounts = [],
  onClose,
  onSave,
  onRefreshStatus,
  onImportAccounts
}: SettingsModalProps) {
  const [draft, setDraft] = useState<AppSettings>(settings)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [vaultSuccess, setVaultSuccess] = useState<string | null>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  useDialogFocus(dialogRef, onClose, !busy)

  const save = async () => {
    try {
      setBusy(true)
      setError(null)
      const interval = Math.round(Number(draft.autoRefreshIntervalMinutes))
      const nextSettings: AppSettings = {
        ...draft,
        autoRefreshIntervalMinutes: Math.min(1440, Math.max(15, Number.isFinite(interval) ? interval : 15)),
        skipUnsupportedRegionRefresh: draft.skipUnsupportedRegionRefresh ?? true
      }

      await onSave(nextSettings)
      await onRefreshStatus()
      onClose()
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  const exportVault = () => {
    try {
      const payload = JSON.stringify({
        version: 1,
        exportedAt: new Date().toISOString(),
        accountCount: accounts.length,
        accounts
      }, null, 2)
      const blob = new Blob([payload], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = `codex_accounts_backup_${new Date().toISOString().slice(0, 10)}.json`
      document.body.appendChild(link)
      link.click()
      document.body.removeChild(link)
      URL.revokeObjectURL(url)
      setVaultSuccess('Backup exported successfully')
      setTimeout(() => setVaultSuccess(null), 3000)
    } catch (err) {
      setError(`Export failed: ${err}`)
    }
  }

  const importVault = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    if (!file) return
    try {
      setBusy(true)
      setError(null)
      const text = await file.text()
      const data = JSON.parse(text)
      if (!Array.isArray(data.accounts)) {
        throw new Error('Invalid backup file: missing accounts list')
      }
      if (onImportAccounts) {
        await onImportAccounts(data.accounts)
      }
      setVaultSuccess(`Imported ${data.accounts.length} accounts`)
      setTimeout(() => setVaultSuccess(null), 3000)
    } catch (err) {
      setError(`Import failed: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusy(false)
      if (event.target) event.target.value = ''
    }
  }

  return (
    <div className="settings-backdrop">
      <div
        ref={dialogRef}
        className="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-dialog-title"
        aria-busy={busy}
        tabIndex={-1}
      >
        <div className="settings-dialog-header">
          <div id="settings-dialog-title" className="settings-dialog-title">
            <Settings size={17} aria-hidden="true" />
            Settings
          </div>
          <button
            type="button"
            className="settings-close"
            onClick={onClose}
            disabled={busy}
            title="Close settings"
            aria-label="Close settings"
          >
            <X size={15} aria-hidden="true" />
          </button>
        </div>

        <div className="settings-dialog-body">
          {/* Auto Refresh */}
          <div className="settings-card">
            <div
              className="settings-card-header"
              onClick={() => setDraft((prev) => ({ ...prev, autoRefreshEnabled: !prev.autoRefreshEnabled }))}
            >
              <span className="settings-card-label">Auto-refresh quotas</span>
              <Switch
                id="auto-refresh-enabled"
                checked={draft.autoRefreshEnabled}
                onChange={(checked) => setDraft((prev) => ({ ...prev, autoRefreshEnabled: checked }))}
                ariaLabel="Auto-refresh quotas"
              />
            </div>

            {draft.autoRefreshEnabled && (
              <div className="settings-card-content">
                <div className="settings-interval-group">
                  <div className="settings-quick-pills" role="group" aria-label="Quick refresh intervals">
                    {quickIntervals.map(({ label, minutes }) => (
                      <button
                        key={minutes}
                        type="button"
                        className="settings-pill"
                        aria-pressed={draft.autoRefreshIntervalMinutes === minutes}
                        onClick={(e) => {
                          e.stopPropagation()
                          setDraft((prev) => ({ ...prev, autoRefreshIntervalMinutes: minutes }))
                        }}
                      >
                        {label}
                      </button>
                    ))}
                  </div>

                  <div className="settings-interval-custom" onClick={(e) => e.stopPropagation()}>
                    <input
                      id="refresh-interval"
                      type="number"
                      min={15}
                      max={1440}
                      step={1}
                      className="settings-number-input"
                      value={draft.autoRefreshIntervalMinutes}
                      onChange={(event) => setDraft((prev) => ({
                        ...prev,
                        autoRefreshIntervalMinutes: Number(event.target.value)
                      }))}
                      aria-label="Custom refresh interval in minutes"
                    />
                    <span className="settings-interval-unit">min</span>
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* Close to Tray */}
          <div
            className="settings-card settings-card-header"
            onClick={() => setDraft((prev) => ({ ...prev, closeToTray: !prev.closeToTray }))}
          >
            <span className="settings-card-label">Close to tray</span>
            <Switch
              id="close-to-tray"
              checked={draft.closeToTray}
              onChange={(checked) => setDraft((prev) => ({ ...prev, closeToTray: checked }))}
              ariaLabel="Close to tray"
            />
          </div>

          {/* Antigravity Target Surfaces */}
          <div className="settings-card">
            <div className="settings-card-header">
              <span className="settings-card-label">Antigravity switch targets</span>
            </div>
            <div className="settings-card-content border-t border-white/[0.04] pt-3 mt-1.5 flex flex-col gap-2">
              <p className="text-xs text-ag-muted leading-relaxed">
                Choose which environments switch active accounts when activating a Gemini account.
              </p>
              <div className="flex flex-col gap-2 pt-1">
                {[
                  { id: 'antigravity', label: 'Antigravity (Desktop App)', desc: 'Restarts Antigravity.exe' },
                  { id: 'ide', label: 'Antigravity IDE', desc: 'Restarts Antigravity IDE.exe' },
                  { id: 'cli', label: 'Antigravity CLI', desc: 'Updates credentials for agy terminal' }
                ].map((surface) => {
                  const currentTargets = draft.geminiSwitchTargets ?? ['antigravity', 'ide', 'cli']
                  const isChecked = currentTargets.includes(surface.id)
                  const isOnly = isChecked && currentTargets.length <= 1

                  return (
                    <div
                      key={surface.id}
                      className="flex items-center justify-between py-1 px-1.5 rounded-lg hover:bg-white/[0.02] cursor-pointer"
                      onClick={() => {
                        if (isChecked && isOnly) return
                        const next = isChecked
                          ? currentTargets.filter((t) => t !== surface.id)
                          : [...currentTargets, surface.id]
                        setDraft((prev) => ({ ...prev, geminiSwitchTargets: next }))
                      }}
                    >
                      <div className="flex flex-col">
                        <span className="text-xs font-medium text-ag-text">{surface.label}</span>
                        <span className="text-[11px] text-ag-muted">{surface.desc}</span>
                      </div>
                      <Switch
                        id={`surface-${surface.id}`}
                        checked={isChecked}
                        disabled={isChecked && isOnly}
                        onChange={(checked) => {
                          if (!checked && isOnly) return
                          const next = checked
                            ? [...currentTargets, surface.id]
                            : currentTargets.filter((t) => t !== surface.id)
                          setDraft((prev) => ({ ...prev, geminiSwitchTargets: next }))
                        }}
                        ariaLabel={surface.label}
                      />
                    </div>
                  )
                })}
              </div>
            </div>
          </div>

          {/* Backup & Vault */}
          <div className="settings-card">
            <div className="settings-card-header">
              <span className="settings-card-label">Backup & Profiles Vault</span>
            </div>
            <div className="settings-card-content border-t border-white/[0.04] pt-3 mt-1.5 flex flex-col gap-2.5">
              <p className="text-xs text-ag-muted leading-relaxed">
                Export and import all saved account credentials and quota settings to transfer them securely between devices.
              </p>
              <div className="flex items-center gap-2 pt-1 flex-wrap">
                <button
                  type="button"
                  onClick={exportVault}
                  className="h-8 px-3 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface inline-flex items-center gap-1.5 transition-all"
                >
                  <Download size={13} />
                  Export backup ({accounts.length})
                </button>
                <label className="h-8 px-3 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface inline-flex items-center gap-1.5 cursor-pointer transition-all">
                  <Upload size={13} />
                  Import backup
                  <input
                    type="file"
                    accept=".json"
                    onChange={(e) => void importVault(e)}
                    className="hidden"
                  />
                </label>
              </div>
              {vaultSuccess && (
                <div className="text-xs text-emerald-400 font-medium animate-fade-in">
                  {vaultSuccess}
                </div>
              )}
            </div>
          </div>

          {error && (
            <div className="settings-error" role="alert">
              {error}
            </div>
          )}
        </div>

        <div className="settings-dialog-footer">
          <button
            type="button"
            className="settings-cancel"
            onClick={onClose}
            disabled={busy}
          >
            Cancel
          </button>
          <button
            type="button"
            className="settings-save"
            onClick={() => void save()}
            disabled={busy}
          >
            {busy ? <Loader2 size={15} className="animate-spin" aria-hidden="true" /> : 'Save'}
          </button>
        </div>
      </div>
    </div>
  )
}
