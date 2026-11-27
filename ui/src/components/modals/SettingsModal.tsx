import { useRef, useState } from 'react'
import { ChevronDown, ChevronUp, Loader2, Settings, X } from 'lucide-react'
import type { Account, AccountProvider, AppData, AppSettings, AutoRefreshStatus, GeminiQuotaFocus } from '../../types'
import { useDialogFocus } from '../../hooks/useDialogFocus'
import { Switch } from '../common/Switch'

type SettingsModalProps = {
  settings: AppSettings
  accounts?: Account[]
  onClose: () => void
  onSave: (settings: AppSettings) => Promise<AppData | null>
  onRefreshStatus: () => Promise<AutoRefreshStatus>
  appVersion?: string | null
}

const geminiQuotaFocusOptions: Array<{ value: GeminiQuotaFocus; label: string; title: string }> = [
  { value: 'gemini', label: 'Gemini', title: 'Recommend by the Gemini models limits' },
  { value: 'claude_and_gpt', label: 'Claude & GPT', title: 'Recommend by the Claude and GPT models limits' },
  { value: 'both', label: 'Both', title: 'Recommend by whichever model group is tighter' }
]

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
  appVersion
}: SettingsModalProps) {
  const [draft, setDraft] = useState<AppSettings>(() => ({
    ...settings,
    enabledProviders: settings.enabledProviders ?? ['codex', 'gemini'],
    privacyMode: settings.privacyMode ?? false
  }))
  const [intervalText, setIntervalText] = useState(() => String(settings.autoRefreshIntervalMinutes ?? 15))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  useDialogFocus(dialogRef, onClose, !busy)

  const save = async () => {
    try {
      setBusy(true)
      setError(null)
      const raw = parseInt(intervalText, 10)
      const interval = Math.min(1440, Math.max(15, isNaN(raw) ? (Number(draft.autoRefreshIntervalMinutes) || 15) : raw))
      const nextSettings: AppSettings = {
        ...draft,
        accountTags: settings.accountTags,
        autoRefreshIntervalMinutes: interval,
        skipUnsupportedRegionRefresh: draft.skipUnsupportedRegionRefresh ?? true,
        enabledProviders: draft.enabledProviders ?? ['codex', 'gemini'],
        privacyMode: draft.privacyMode ?? false
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

  return (
    <div
      className="settings-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onClose()
      }}
    >
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
                        onClick={() => {
                          setDraft((prev) => ({
                            ...prev,
                            autoRefreshIntervalMinutes: minutes
                          }))
                          setIntervalText(String(minutes))
                        }}
                      >
                        {label}
                      </button>
                    ))}
                  </div>

                  <div className="settings-interval-custom" onClick={(e) => e.stopPropagation()}>
                    <div className="settings-stepper-box">
                      <input
                        id="refresh-interval"
                        type="number"
                        min={15}
                        max={1440}
                        step={1}
                        className="settings-number-input"
                        value={intervalText}
                        onChange={(event) => {
                          setIntervalText(event.target.value)
                          const num = parseInt(event.target.value, 10)
                          if (!isNaN(num)) {
                            setDraft((prev) => ({
                              ...prev,
                              autoRefreshIntervalMinutes: num
                            }))
                          }
                        }}
                        onBlur={() => {
                          const num = parseInt(intervalText, 10)
                          const clamped = Math.min(1440, Math.max(15, isNaN(num) ? 15 : num))
                          setDraft((prev) => ({
                            ...prev,
                            autoRefreshIntervalMinutes: clamped
                          }))
                          setIntervalText(String(clamped))
                        }}
                        aria-label="Custom refresh interval in minutes"
                      />
                      <div className="settings-stepper-controls">
                        <button
                          type="button"
                          className="settings-stepper-btn"
                          tabIndex={-1}
                          onClick={() => {
                            const currentVal = parseInt(intervalText, 10) || 15
                            const nextVal = Math.min(1440, currentVal + 1)
                            setDraft((prev) => ({
                              ...prev,
                              autoRefreshIntervalMinutes: nextVal
                            }))
                            setIntervalText(String(nextVal))
                          }}
                          title="Increase interval"
                          aria-label="Increase interval"
                        >
                          <ChevronUp size={10} aria-hidden="true" />
                        </button>
                        <button
                          type="button"
                          className="settings-stepper-btn"
                          tabIndex={-1}
                          onClick={() => {
                            const currentVal = parseInt(intervalText, 10) || 15
                            const nextVal = Math.max(15, currentVal - 1)
                            setDraft((prev) => ({
                              ...prev,
                              autoRefreshIntervalMinutes: nextVal
                            }))
                            setIntervalText(String(nextVal))
                          }}
                          title="Decrease interval"
                          aria-label="Decrease interval"
                        >
                          <ChevronDown size={10} aria-hidden="true" />
                        </button>
                      </div>
                    </div>
                    <span className="settings-interval-unit">min</span>
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* AI Providers & Tabs */}
          <div className="settings-card">
            <div className="settings-card-header">
              <span className="settings-card-label">Visible tabs</span>
            </div>
            <div className="settings-card-content border-t border-white/[0.04] pt-1 mt-1 flex flex-col divide-y divide-white/[0.04]">
              {/* ChatGPT Row */}
              {(() => {
                const enabled = (draft.enabledProviders ?? ['codex', 'gemini']).includes('codex')
                const isOnly = enabled && (draft.enabledProviders ?? ['codex', 'gemini']).length <= 1
                const count = accounts.filter((a) => (a.provider ?? 'codex') === 'codex').length

                const toggle = () => {
                  if (isOnly) return
                  const current = draft.enabledProviders ?? ['codex', 'gemini']
                  const next = enabled
                    ? (current.filter((p) => p !== 'codex') as AccountProvider[])
                    : ([...current.filter((p) => p !== 'codex'), 'codex'] as AccountProvider[])
                  setDraft((prev) => ({ ...prev, enabledProviders: next }))
                }

                return (
                  <div
                    className={`flex items-center justify-between py-2 px-2 -mx-2 rounded-lg select-none transition-all ${
                      isOnly
                        ? 'cursor-not-allowed opacity-60'
                        : 'cursor-pointer hover:bg-white/[0.04] hover:text-white active:bg-white/[0.07]'
                    }`}
                    onClick={toggle}
                    title={isOnly ? 'At least one provider tab must remain enabled' : undefined}
                  >
                    <div className="flex items-center gap-2">
                      <span className={`text-xs font-medium ${enabled ? 'text-ag-text' : 'text-ag-muted'}`}>
                        ChatGPT
                      </span>
                      <span className="text-[11px] text-ag-muted">
                        ({count} {count === 1 ? 'account' : 'accounts'})
                      </span>
                    </div>
                    <Switch
                      id="provider-switch-codex"
                      checked={enabled}
                      onChange={toggle}
                      disabled={isOnly}
                      ariaLabel="Enable ChatGPT tab"
                    />
                  </div>
                )
              })()}

              {/* Gemini Row */}
              {(() => {
                const enabled = (draft.enabledProviders ?? ['codex', 'gemini']).includes('gemini')
                const isOnly = enabled && (draft.enabledProviders ?? ['codex', 'gemini']).length <= 1
                const count = accounts.filter((a) => a.provider === 'gemini').length

                const toggle = () => {
                  if (isOnly) return
                  const current = draft.enabledProviders ?? ['codex', 'gemini']
                  const next = enabled
                    ? (current.filter((p) => p !== 'gemini') as AccountProvider[])
                    : ([...current.filter((p) => p !== 'gemini'), 'gemini'] as AccountProvider[])
                  setDraft((prev) => ({ ...prev, enabledProviders: next }))
                }

                return (
                  <div
                    className={`flex items-center justify-between py-2 px-2 -mx-2 rounded-lg select-none transition-all ${
                      isOnly
                        ? 'cursor-not-allowed opacity-60'
                        : 'cursor-pointer hover:bg-white/[0.04] hover:text-white active:bg-white/[0.07]'
                    }`}
                    onClick={toggle}
                    title={isOnly ? 'At least one provider tab must remain enabled' : undefined}
                  >
                    <div className="flex items-center gap-2">
                      <span className={`text-xs font-medium ${enabled ? 'text-ag-text' : 'text-ag-muted'}`}>
                        Gemini
                      </span>
                      <span className="text-[11px] text-ag-muted">
                        ({count} {count === 1 ? 'account' : 'accounts'})
                      </span>
                    </div>
                    <Switch
                      id="provider-switch-gemini"
                      checked={enabled}
                      onChange={toggle}
                      disabled={isOnly}
                      ariaLabel="Enable Gemini tab"
                    />
                  </div>
                )
              })()}
            </div>
          </div>

          {(draft.enabledProviders ?? ['codex', 'gemini']).includes('gemini') && (
            <div className="settings-card">
              <div className="settings-card-header">
                <span className="settings-card-label">Antigravity recommendation uses</span>
              </div>
              <div className="settings-card-content">
                <div className="settings-quick-pills" role="group" aria-label="Antigravity model group for recommendations">
                  {geminiQuotaFocusOptions.map(({ value, label, title }) => (
                    <button
                      key={value}
                      type="button"
                      className="settings-pill"
                      title={title}
                      aria-pressed={(draft.geminiQuotaFocus ?? 'gemini') === value}
                      onClick={() => setDraft((prev) => ({ ...prev, geminiQuotaFocus: value }))}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}

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


          {error && (
            <div className="settings-error" role="alert">
              {error}
            </div>
          )}
        </div>

        <div className="settings-dialog-footer">
          {appVersion && <span className="settings-version">SwitchAI {appVersion}</span>}
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
