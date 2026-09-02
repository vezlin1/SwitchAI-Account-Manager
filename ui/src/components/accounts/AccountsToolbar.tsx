import { ArrowDownToLine, Loader2, LogIn, RefreshCw, Search, UserPlus, X } from 'lucide-react'
import type { AccountProvider, AppSettings, AutoRefreshStatus } from '../../types'

type AccountsToolbarProps = {
  accountCount: number
  provider?: AccountProvider
  refreshingAll: boolean
  addingAccount: boolean
  importingAntigravity?: boolean
  appSettings: AppSettings
  autoRefreshStatus: AutoRefreshStatus | null
  searchTerm?: string
  onSearchChange?: (value: string) => void
  onAddAccount: () => void
  onCancelAddAccount: () => void
  onImportAntigravity?: () => void
  onRefreshAll: () => void
}

function autoRefreshLabel(settings: AppSettings, status: AutoRefreshStatus | null): string {
  if (!settings.autoRefreshEnabled) return 'off'
  if (status?.inFlight) return 'updating…'
  return `active (${settings.autoRefreshIntervalMinutes}m)`
}

export function AccountsToolbar({
  accountCount,
  provider,
  refreshingAll,
  addingAccount,
  importingAntigravity = false,
  appSettings,
  autoRefreshStatus,
  searchTerm = '',
  onSearchChange,
  onAddAccount,
  onCancelAddAccount,
  onImportAntigravity,
  onRefreshAll
}: AccountsToolbarProps) {
  const isGoogle = provider === 'gemini'
  const accountActionBusy = addingAccount || importingAntigravity

  return (
    <div className="accounts-toolbar flex items-center justify-between gap-3 flex-wrap">
      <div className="accounts-toolbar-primary inline-flex items-center gap-2 flex-wrap">
        <button
          type="button"
          className="h-9 px-3.5 rounded-lg bg-ag-primary text-white text-xs font-semibold hover:bg-blue-600 inline-flex items-center gap-2 disabled:opacity-60 transition-all shadow-sm"
          onClick={onAddAccount}
          disabled={accountActionBusy || refreshingAll}
          title={isGoogle ? 'Add an Antigravity account with Google OAuth' : 'Add a ChatGPT account'}
        >
          {addingAccount
            ? <Loader2 size={15} className="animate-spin" aria-hidden="true" />
            : isGoogle
              ? <LogIn size={15} aria-hidden="true" />
              : <UserPlus size={15} aria-hidden="true" />}
          {addingAccount
            ? (isGoogle ? 'Waiting for Google…' : 'Waiting for sign-in…')
            : (isGoogle ? 'Sign in with Google' : 'Add account')}
        </button>

        {addingAccount && (
          <button
            className="h-9 w-9 inline-flex items-center justify-center rounded-lg border border-ag-border text-ag-muted hover:text-ag-text hover:bg-ag-surface transition-all"
            onClick={onCancelAddAccount}
            title="Cancel sign-in"
            aria-label="Cancel sign-in"
            type="button"
          >
            <X size={15} aria-hidden="true" />
          </button>
        )}

        {isGoogle && onImportAntigravity && (
          <button
            type="button"
            className="h-9 px-3.5 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface inline-flex items-center gap-2 disabled:opacity-60 transition-all"
            onClick={onImportAntigravity}
            disabled={accountActionBusy || refreshingAll}
            title="Import the Google account currently active in Antigravity"
          >
            {importingAntigravity
              ? <Loader2 size={15} className="animate-spin" aria-hidden="true" />
              : <ArrowDownToLine size={15} aria-hidden="true" />}
            {importingAntigravity ? 'Importing…' : 'Import current session'}
          </button>
        )}

        <button
          type="button"
          className="h-9 px-3.5 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface inline-flex items-center gap-2 transition-all"
          onClick={onRefreshAll}
          disabled={refreshingAll || autoRefreshStatus?.inFlight || accountCount === 0}
        >
          {refreshingAll
            ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
            : <RefreshCw size={14} aria-hidden="true" />}
          Refresh
        </button>

        {onSearchChange && (
          <div className="relative inline-flex items-center min-w-[180px] max-w-[280px]">
            <Search size={14} className="absolute left-2.5 text-ag-muted pointer-events-none" />
            <input
              type="text"
              placeholder="Search accounts..."
              value={searchTerm}
              onChange={(e) => onSearchChange(e.target.value)}
              className="h-9 w-full pl-8 pr-7 rounded-lg bg-ag-surface/80 border border-ag-border text-xs text-ag-text placeholder:text-ag-muted/60 focus:outline-none focus:border-blue-500 transition-colors"
            />
            {searchTerm && (
              <button
                type="button"
                onClick={() => onSearchChange('')}
                className="absolute right-2 text-ag-muted hover:text-ag-text p-0.5"
                title="Clear search"
              >
                <X size={13} />
              </button>
            )}
          </div>
        )}

        {addingAccount && (
          <span className="text-xs text-ag-muted ml-2" role="status" aria-live="polite">
            Complete sign-in in your browser. This window will update automatically.
          </span>
        )}
      </div>

      <div className="accounts-toolbar-status ml-auto text-xs text-ag-muted flex items-center gap-2.5 flex-wrap justify-end">
        <div
          className="inline-flex items-center gap-1.5 cursor-default select-none"
          title={
            autoRefreshStatus?.lastFinishedAt
              ? `Last updated: ${new Date(autoRefreshStatus.lastFinishedAt * 1000).toLocaleTimeString()}`
              : 'Auto-refresh active'
          }
        >
          <span
            className={`w-1.5 h-1.5 rounded-full ${
              !appSettings.autoRefreshEnabled
                ? 'bg-zinc-600'
                : autoRefreshStatus?.inFlight
                  ? 'bg-ag-primary'
                  : 'bg-ag-success'
            }`}
          />
          <span className="text-ag-muted">Auto-refresh:</span>
          <span className="text-ag-text font-medium">{autoRefreshLabel(appSettings, autoRefreshStatus)}</span>
        </div>
      </div>
    </div>
  )
}
