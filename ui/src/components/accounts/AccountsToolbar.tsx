import { useEffect, useRef, useState } from 'react'
import { ArrowDownToLine, ChevronDown, Loader2, LogIn, RefreshCw, Search, UserPlus, X } from 'lucide-react'
import type { AccountProvider, AppSettings, AutoRefreshStatus } from '../../types'

type AccountsToolbarProps = {
  accountCount: number
  provider?: AccountProvider
  refreshingAll: boolean
  addingAccount: boolean
  importingSession?: boolean
  importingAntigravity?: boolean
  appSettings: AppSettings
  autoRefreshStatus: AutoRefreshStatus | null
  searchTerm?: string
  onSearchChange?: (value: string) => void
  onAddAccount: () => void
  onCancelAddAccount: () => void
  onImportSession?: () => void
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
  importingSession,
  importingAntigravity = false,
  appSettings,
  autoRefreshStatus,
  searchTerm = '',
  onSearchChange,
  onAddAccount,
  onCancelAddAccount,
  onImportSession,
  onImportAntigravity,
  onRefreshAll
}: AccountsToolbarProps) {
  const isGoogle = provider === 'gemini'
  const isImporting = importingSession ?? importingAntigravity
  const accountActionBusy = addingAccount || isImporting
  const handleImport = onImportSession ?? onImportAntigravity
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!dropdownOpen) return

    const handlePointerDown = (event: PointerEvent) => {
      if (!dropdownRef.current?.contains(event.target as Node)) {
        setDropdownOpen(false)
      }
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setDropdownOpen(false)
      }
    }

    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [dropdownOpen])

  return (
    <div className="accounts-toolbar flex items-center justify-between gap-3 flex-wrap">
      <div className="accounts-toolbar-primary inline-flex items-center gap-2 flex-wrap">
        {handleImport ? (
          <div className="relative inline-flex items-center" ref={dropdownRef}>
            <button
              type="button"
              className="h-9 px-3.5 rounded-lg bg-ag-primary text-white text-xs font-semibold hover:bg-blue-600 active:bg-blue-700 active:scale-[0.98] inline-flex items-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed disabled:active:scale-100 transition-all shadow-sm cursor-pointer select-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-400"
              onClick={() => {
                if (accountActionBusy) return
                setDropdownOpen((prev) => !prev)
              }}
              disabled={accountActionBusy || refreshingAll}
              aria-haspopup="menu"
              aria-expanded={dropdownOpen}
              title={isGoogle ? 'Add a Gemini account' : 'Add a ChatGPT account'}
            >
              {accountActionBusy ? (
                <Loader2 size={15} className="animate-spin" aria-hidden="true" />
              ) : (
                <UserPlus size={15} aria-hidden="true" />
              )}
              <span>
                {addingAccount
                  ? (isGoogle ? 'Waiting for Google…' : 'Waiting for sign-in…')
                  : isImporting
                    ? 'Importing session…'
                    : 'Add Account'}
              </span>
              {!accountActionBusy && (
                <ChevronDown
                  size={13}
                  className={`transition-transform duration-150 opacity-80 ${dropdownOpen ? 'rotate-180' : ''}`}
                  aria-hidden="true"
                />
              )}
            </button>

            {dropdownOpen && !accountActionBusy && (
              <div
                className="absolute left-0 top-[calc(100%+6px)] z-50 min-w-[270px] rounded-xl border border-white/[0.08] bg-[#0c1017]/95 backdrop-blur-md shadow-2xl p-1.5 flex flex-col gap-1 origin-top-left animate-in fade-in-0 zoom-in-95 duration-100"
                role="menu"
              >
                <button
                  type="button"
                  role="menuitem"
                  className="flex items-start gap-2.5 p-2 rounded-lg text-left hover:bg-white/[0.08] focus-visible:bg-white/[0.08] focus-visible:outline-none active:bg-white/[0.12] active:scale-[0.99] transition-all cursor-pointer group select-none"
                  onClick={() => {
                    setDropdownOpen(false)
                    onAddAccount()
                  }}
                >
                  <div className="p-1.5 rounded-md bg-blue-500/10 text-blue-400 group-hover:bg-blue-500/20 group-hover:text-blue-300 transition-colors mt-0.5 shrink-0">
                    <LogIn size={15} />
                  </div>
                  <div className="flex flex-col min-w-0">
                    <span className="text-xs font-semibold text-ag-text group-hover:text-white">
                      {isGoogle ? 'Sign in with Google' : 'Sign in with ChatGPT'}
                    </span>
                    <span className="text-[11px] text-ag-muted leading-snug">
                      Authorize account via browser OAuth
                    </span>
                  </div>
                </button>

                <button
                  type="button"
                  role="menuitem"
                  className="flex items-start gap-2.5 p-2 rounded-lg text-left hover:bg-white/[0.08] focus-visible:bg-white/[0.08] focus-visible:outline-none active:bg-white/[0.12] active:scale-[0.99] transition-all cursor-pointer group select-none"
                  onClick={() => {
                    setDropdownOpen(false)
                    handleImport()
                  }}
                >
                  <div className="p-1.5 rounded-md bg-emerald-500/10 text-emerald-400 group-hover:bg-emerald-500/20 group-hover:text-emerald-300 transition-colors mt-0.5 shrink-0">
                    <ArrowDownToLine size={15} />
                  </div>
                  <div className="flex flex-col min-w-0">
                    <span className="text-xs font-semibold text-ag-text group-hover:text-white">
                      Import current session
                    </span>
                    <span className="text-[11px] text-ag-muted leading-snug">
                      {isGoogle
                        ? 'Import active Antigravity credentials'
                        : 'Import active session from ~/.codex/auth.json'}
                    </span>
                  </div>
                </button>
              </div>
            )}
          </div>
        ) : (
          <button
            type="button"
            className="h-9 px-3.5 rounded-lg bg-ag-primary text-white text-xs font-semibold hover:bg-blue-600 active:bg-blue-700 active:scale-[0.98] inline-flex items-center gap-2 disabled:opacity-60 disabled:cursor-not-allowed disabled:active:scale-100 transition-all shadow-sm cursor-pointer select-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-400"
            onClick={onAddAccount}
            disabled={accountActionBusy || refreshingAll}
            title={isGoogle ? 'Add a Gemini account' : 'Add a ChatGPT account'}
          >
            {addingAccount ? (
              <Loader2 size={15} className="animate-spin" aria-hidden="true" />
            ) : (
              <UserPlus size={15} aria-hidden="true" />
            )}
            {addingAccount ? 'Waiting for sign-in…' : 'Add account'}
          </button>
        )}

        {accountActionBusy && (
          <button
            className="h-9 w-9 inline-flex items-center justify-center rounded-lg border border-ag-border text-ag-muted hover:text-ag-text hover:bg-ag-surface hover:border-white/20 active:scale-95 active:bg-ag-surface/80 transition-all cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ag-primary"
            onClick={onCancelAddAccount}
            title="Cancel"
            aria-label="Cancel"
            type="button"
          >
            <X size={15} aria-hidden="true" />
          </button>
        )}

        <button
          type="button"
          className="h-9 px-3.5 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface hover:border-white/20 hover:text-white active:scale-[0.98] active:bg-ag-surface/80 inline-flex items-center gap-2 transition-all cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:border-ag-border disabled:hover:bg-transparent disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ag-primary select-none"
          onClick={onRefreshAll}
          disabled={refreshingAll || autoRefreshStatus?.inFlight || accountCount === 0}
        >
          {refreshingAll
            ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
            : <RefreshCw size={14} aria-hidden="true" />}
          Refresh
        </button>

        {onSearchChange && (
          <div className="account-search-wrapper">
            <Search size={14} className="absolute left-2.5 text-ag-muted pointer-events-none" />
            <input
              type="text"
              placeholder="Search accounts..."
              value={searchTerm}
              onChange={(e) => onSearchChange(e.target.value)}
              className="account-search-input"
            />
            {searchTerm && (
              <button
                type="button"
                onClick={() => onSearchChange('')}
                className="account-search-clear"
                title="Clear search"
                aria-label="Clear search"
              >
                <X size={12} strokeWidth={2} />
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
