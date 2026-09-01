import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Loader2, Maximize2, Minimize2, Minus, Settings, X } from 'lucide-react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getVersion } from '@tauri-apps/api/app'
import './App.css'
import appLogo from './assets/app-icon.png'
import { ErrorBanner } from './components/common'
import { AccountsTab } from './components/accounts/AccountsTab'
import { useAutoRefreshStatus } from './hooks/useAutoRefreshStatus'
import { useAppData } from './hooks/useAppData'
import { usePlatform } from './hooks/usePlatform'
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts'
import type { Account, AccountProvider } from './types'

const RecoveryModal = lazy(() =>
  import('./components/modals/RecoveryModal').then((m) => ({ default: m.RecoveryModal }))
)
const SettingsModal = lazy(() =>
  import('./components/modals/SettingsModal').then((m) => ({ default: m.SettingsModal }))
)

const appWindow = getCurrentWindow()

function App() {
  const {
    data,
    setData,
    getData,
    saveAppSettings,
    loading,
    error,
    clearError,
    reload,
    reloadAccountOrder,
    startup,
    recovery,
    startupWarnings,
    startupLoading,
    restoreStateBackup,
    startFresh,
    openRecoveryDataDirectory
  } = useAppData()
  const autoRefresh = useAutoRefreshStatus()
  const { isMac } = usePlatform()
  const [activeProvider, setActiveProvider] = useState<AccountProvider>(() => {
    if (typeof window !== 'undefined') {
      try {
        const cached = localStorage.getItem('switchai:last-active-provider')
        if (cached === 'codex' || cached === 'gemini') return cached
      } catch {
        // ignore
      }
    }
    return 'codex'
  })
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [isMaximized, setIsMaximized] = useState(false)
  const [appVersion, setAppVersion] = useState<string | null>(null)
  const errorRef = useRef<HTMLDivElement>(null)

  const syncedProviderRef = useRef<AccountProvider | null>(null)

  useEffect(() => {
    const serverProv = data?.appSettings?.lastActiveProvider
    if (serverProv && syncedProviderRef.current !== serverProv) {
      syncedProviderRef.current = serverProv
      setActiveProvider(serverProv)
      try {
        localStorage.setItem('switchai:last-active-provider', serverProv)
      } catch {
        // ignore
      }
    }
  }, [data?.appSettings?.lastActiveProvider])

  const handleSelectProvider = useCallback(
    (provider: AccountProvider) => {
      setActiveProvider(provider)
      syncedProviderRef.current = provider
      try {
        localStorage.setItem('switchai:last-active-provider', provider)
      } catch {
        // ignore
      }
      if (data && data.appSettings.lastActiveProvider !== provider) {
        void saveAppSettings({
          ...data.appSettings,
          lastActiveProvider: provider
        })
      }
    },
    [data, saveAppSettings]
  )

  useEffect(() => {
    void getVersion().then(setAppVersion).catch(() => setAppVersion(null))
    void appWindow.show().catch(() => undefined)
  }, [])

  useEffect(() => {
    if (!loading && !data && error) {
      errorRef.current?.focus()
    }
  }, [data, error, loading])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    const syncMaximizedState = () => {
      void appWindow.isMaximized().then((maximized) => {
        if (!disposed) setIsMaximized(maximized)
      }).catch(() => undefined)
    }

    syncMaximizedState()
    void appWindow.onResized(syncMaximizedState).then((stopListening) => {
      if (disposed) stopListening()
      else unlisten = stopListening
    }).catch(() => undefined)

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  const lastFocusReloadRef = useRef(0)

  useEffect(() => {
    let unlistenFocus: (() => void) | undefined
    void appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        const now = Date.now()
        if (now - lastFocusReloadRef.current > 60_000) {
          lastFocusReloadRef.current = now
          void reload()
        }
      }
    }).then((stop) => {
      unlistenFocus = stop
    }).catch(() => undefined)

    return () => {
      unlistenFocus?.()
    }
  }, [reload])

  const minimizeWindow = async () => {
    try {
      await appWindow.minimize()
    } catch {
      // The native window may already be closing.
    }
  }

  const closeWindow = async () => {
    try {
      await appWindow.close()
    } catch {
      // The native window may already be closing.
    }
  }

  const toggleMaximizeWindow = async () => {
    try {
      await appWindow.toggleMaximize()
      setIsMaximized(await appWindow.isMaximized())
    } catch {
      // Ignore teardown races from native window controls.
    }
  }

  // Global Keyboard Shortcuts
  const isModalOpen = settingsOpen || startup?.mode === 'recovery_required'
  const shortcuts = useMemo(
    () => ({
      'mod+,': () => setSettingsOpen((prev) => !prev),
      'mod+1': () => handleSelectProvider('codex'),
      'mod+2': () => handleSelectProvider('gemini'),
      'mod+w': () => void closeWindow()
    }),
    [handleSelectProvider]
  )
  useKeyboardShortcuts(shortcuts, !isModalOpen)

  const handleImportAccounts = useCallback(
    async (importedAccounts: Account[]) => {
      if (!data) return
      const existingIds = new Set(data.accounts.map((a) => a.id))
      const uniqueImported = importedAccounts.filter((a) => !existingIds.has(a.id))
      if (uniqueImported.length === 0) return

      const updated = setData((previous) => ({
        ...previous,
        accounts: [...previous.accounts, ...uniqueImported]
      }))
      if (updated) {
        void saveAppSettings(updated.appSettings)
        void reload()
      }
    },
    [data, setData, saveAppSettings, reload]
  )

  return (
    <div className="app-outer h-full w-full text-ag-text">
      <a className="skip-link" href="#main-content">Skip to accounts</a>
      {startup?.mode === 'recovery_required' && recovery && (
        <Suspense fallback={null}>
          <RecoveryModal
            recovery={recovery}
            loading={startupLoading}
            onRestore={restoreStateBackup}
            onStartFresh={startFresh}
            onOpenDataDirectory={openRecoveryDataDirectory}
          />
        </Suspense>
      )}
      {data && settingsOpen && (
        <Suspense fallback={null}>
          <SettingsModal
            settings={data.appSettings}
            accounts={data.accounts}
            status={autoRefresh.status}
            onClose={() => setSettingsOpen(false)}
            onSave={saveAppSettings}
            onRefreshStatus={autoRefresh.refreshStatus}
            onImportAccounts={handleImportAccounts}
          />
        </Suspense>
      )}

      <div className="app-shell h-full w-full flex flex-col">
        <header
          className="app-header w-full flex items-center justify-between sticky top-0 z-30 select-none"
          data-tauri-drag-region
        >
          {/* Left: Branding */}
          <div
            className={`flex items-center gap-2.5 min-w-0 ${isMac ? 'pl-[76px]' : 'pl-4'}`}
            data-tauri-drag-region
          >
            <img
              src={appLogo}
              alt="SwitchAI"
              className="w-5 h-5 rounded-[5px] object-contain shadow-sm pointer-events-none select-none"
              draggable={false}
            />
            <h1 className="app-title font-semibold text-xs tracking-tight text-ag-text truncate" data-tauri-drag-region>
              SwitchAI
            </h1>
            {appVersion !== undefined && (
              <span className="app-version hidden sm:inline-flex" data-tauri-drag-region>
              {appVersion ? `v${appVersion}` : 'v1.0.0'}
              </span>
            )}
          </div>

          {/* Center: Tabs */}
          <div className="flex-1 flex justify-center min-w-0 px-4" data-tauri-drag-region>
            <div className="provider-tabs-nav" role="tablist" aria-label="Provider selection" data-no-drag>
              <button
                type="button"
                role="tab"
                aria-selected={activeProvider === 'codex'}
                className={`provider-tab-pill ${activeProvider === 'codex' ? 'provider-tab-pill-active' : ''}`}
                onClick={() => handleSelectProvider('codex')}
              >
                <span className="provider-tab-title">ChatGPT</span>
                <span className="provider-tab-count">
                  {data?.accounts.filter((a) => (a.provider ?? 'codex') === 'codex').length ?? 0}
                </span>
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={activeProvider === 'gemini'}
                className={`provider-tab-pill ${activeProvider === 'gemini' ? 'provider-tab-pill-active' : ''}`}
                onClick={() => handleSelectProvider('gemini')}
              >
                <span className="provider-tab-title">Gemini</span>
                <span className="provider-tab-count">
                  {data?.accounts.filter((a) => a.provider === 'gemini').length ?? 0}
                </span>
              </button>
            </div>
          </div>

          {/* Right: Actions / Controls */}
          <div className="flex items-center justify-end h-full shrink-0" data-no-drag>
            <button
              className={`header-icon-btn ${isMac ? 'mr-3' : 'mr-1'}`}
              onClick={() => setSettingsOpen(true)}
              title={isMac ? 'Settings (⌘,)' : 'Settings (Ctrl+,)'}
              aria-label="Settings"
            >
              <Settings size={14} />
            </button>

            {!isMac && (
              <div className="flex items-center h-full">
                <button
                  className="win-caption-btn win-caption-minimize"
                  onClick={() => void minimizeWindow()}
                  title="Minimize"
                  aria-label="Minimize"
                >
                  <Minus size={13} strokeWidth={2} />
                </button>
                <button
                  className="win-caption-btn win-caption-maximize"
                  onClick={() => void toggleMaximizeWindow()}
                  title={isMaximized ? 'Restore' : 'Maximize'}
                  aria-label={isMaximized ? 'Restore' : 'Maximize'}
                  aria-pressed={isMaximized}
                >
                  {isMaximized ? (
                    <Minimize2 size={13} />
                  ) : (
                    <Maximize2 size={13} />
                  )}
                </button>
                <button
                  className="win-caption-btn win-caption-close"
                  onClick={() => void closeWindow()}
                  title={data?.appSettings.closeToTray ? 'Hide to tray' : 'Close'}
                  aria-label={data?.appSettings.closeToTray ? 'Hide to tray' : 'Close'}
                >
                  <X size={14} strokeWidth={2} />
                </button>
              </div>
            )}
          </div>
        </header>

        <main id="main-content" className="app-main flex-1 min-h-0 overflow-auto px-6 py-5" tabIndex={-1}>
          <div className="max-w-[1720px] mx-auto h-full">
            {loading && !data && (
              <div className="h-full min-h-[320px] rounded-2xl border border-ag-border bg-ag-card/40 backdrop-blur-sm shadow-ag flex flex-col items-center justify-center gap-4 text-ag-muted py-16" role="status" aria-live="polite">
                <div className="relative flex items-center justify-center">
                  <div className="w-12 h-12 rounded-xl bg-ag-surface/80 border border-ag-border/80 flex items-center justify-center shadow-lg">
                    <img
                      src={appLogo}
              alt="SwitchAI"
                      className="w-7 h-7 object-contain rounded-md"
                      draggable={false}
                    />
                  </div>
                  <div className="absolute -inset-1.5 rounded-2xl border border-blue-500/25 animate-pulse pointer-events-none" />
                </div>
                <div className="flex items-center gap-2 text-ag-text">
                  <Loader2 size={15} className="animate-spin text-blue-400" />
              <span className="text-xs font-semibold tracking-tight">Loading SwitchAI accounts…</span>
                </div>
                <p className="text-[11px] text-ag-muted">Checking authentication status and quota limits</p>
              </div>
            )}

            {!loading && startupWarnings.length > 0 && (
              <div className="startup-warnings" role="status" aria-live="polite">
                {startupWarnings.map((warning, index) => (
                  <div key={`${index}:${warning}`}>{warning}</div>
                ))}
              </div>
            )}

            {!loading && !data && error && (
              <div
                ref={errorRef}
                className="h-full rounded-2xl border border-red-500/40 bg-red-500/10 shadow-ag p-6 text-red-300"
                role="alert"
                tabIndex={-1}
              >
                Error: {error}
              </div>
            )}

            {data && (
              <div className="h-full min-h-0 flex flex-col gap-3">
                {error && (
                  <ErrorBanner
                    message={error}
                    onDismiss={() => {
                      clearError()
                    }}
                  />
                )}
                <div className="min-h-0 flex-1">
                  <AccountsTab
                    data={data}
                    setData={setData}
                    getData={getData}
                    saveAppSettings={saveAppSettings}
                    reload={reload}
                    reloadAccountOrder={reloadAccountOrder}
                    autoRefreshStatus={autoRefresh.status}
                    autoRefreshError={autoRefresh.error}
                    onClearAutoRefreshError={autoRefresh.clearError}
                    activeProvider={activeProvider}
                  />
                </div>
              </div>
            )}
          </div>
        </main>
      </div>
    </div>
  )
}

export default App
