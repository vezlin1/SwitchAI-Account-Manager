import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getVersion } from '@tauri-apps/api/app'
import './App.css'
import appLogo from './assets/app-icon.png'
import { ErrorBanner, TextInputContextMenu } from './components/common'
import { AccountsTab } from './components/accounts/AccountsTab'
import { warmUpAntigravitySurfacesCache } from './utils/antigravitySurfaces'
import { useAutoRefreshStatus } from './hooks/useAutoRefreshStatus'
import { useAppData } from './hooks/useAppData'
import { usePlatform } from './hooks/usePlatform'
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts'
import { usePrivacy } from './context/usePrivacy'
import type { Account, AccountProvider, UpdateCheckResult } from './types'
import { AppTitleBar } from './components/layout/AppTitleBar'
import { UpdateCoordinator } from './components/layout/UpdateCoordinator'
import { StartupRecoveryGate } from './components/layout/StartupRecoveryGate'

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
  const { privacyMode, togglePrivacyMode } = usePrivacy()
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
  const [updateModalOpen, setUpdateModalOpen] = useState(false)
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null)
  const [appVersion, setAppVersion] = useState<string | null>(null)
  const errorRef = useRef<HTMLDivElement>(null)
  const syncedProviderRef = useRef<AccountProvider | null>(null)

  useEffect(() => {
    warmUpAntigravitySurfacesCache()
  }, [])

  const enabledProviders = useMemo<AccountProvider[]>(() => {
    const list = data?.appSettings?.enabledProviders
    if (Array.isArray(list) && list.length > 0) {
      const filtered = list.filter((p): p is AccountProvider => p === 'codex' || p === 'gemini')
      if (filtered.length > 0) return filtered
    }
    return ['codex', 'gemini']
  }, [data?.appSettings?.enabledProviders])

  useEffect(() => {
    if (enabledProviders.length > 0 && !enabledProviders.includes(activeProvider)) {
      const fallback = enabledProviders[0]
      queueMicrotask(() => {
        setActiveProvider(fallback)
      })
      syncedProviderRef.current = fallback
      try {
        localStorage.setItem('switchai:last-active-provider', fallback)
      } catch {
        // ignore
      }
    }
  }, [enabledProviders, activeProvider])

  useEffect(() => {
    const serverProv = data?.appSettings?.lastActiveProvider
    if (serverProv && syncedProviderRef.current !== serverProv && enabledProviders.includes(serverProv)) {
      syncedProviderRef.current = serverProv
      setActiveProvider(serverProv)
      try {
        localStorage.setItem('switchai:last-active-provider', serverProv)
      } catch {
        // ignore
      }
    }
  }, [data?.appSettings?.lastActiveProvider, enabledProviders])

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

  // Global Keyboard Shortcuts
  const isModalOpen = settingsOpen || updateModalOpen || startup?.mode === 'recovery_required'
  const shortcuts = useMemo(
    () => ({
      'mod+,': () => setSettingsOpen((prev) => !prev),
      'mod+1': () => handleSelectProvider('codex'),
      'mod+2': () => handleSelectProvider('gemini'),
      'mod+shift+p': () => togglePrivacyMode(),
      'mod+w': () => void appWindow.close().catch(() => undefined)
    }),
    [handleSelectProvider, togglePrivacyMode]
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
    <div className={`app-outer h-full w-full text-ag-text ${privacyMode ? 'privacy-mode' : ''}`}>
      <a className="skip-link" href="#main-content">Skip to accounts</a>

      <StartupRecoveryGate
        startup={startup}
        recovery={recovery}
        loading={startupLoading}
        onRestore={restoreStateBackup}
        onStartFresh={startFresh}
        onOpenDataDirectory={openRecoveryDataDirectory}
      />

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
            onOpenUpdateModal={(info) => {
              setUpdateInfo(info)
              setUpdateModalOpen(true)
            }}
            appVersion={appVersion}
          />
        </Suspense>
      )}

      <UpdateCoordinator
        updateModalOpen={updateModalOpen}
        setUpdateModalOpen={setUpdateModalOpen}
        updateInfo={updateInfo}
        setUpdateInfo={setUpdateInfo}
      />

      <TextInputContextMenu />

      <div className="app-shell h-full w-full flex flex-col">
        <AppTitleBar
          isMac={isMac}
          appVersion={appVersion}
          enabledProviders={enabledProviders}
          activeProvider={activeProvider}
          accountCounts={{
            codex: data?.accounts.filter((a) => (a.provider ?? 'codex') === 'codex').length ?? 0,
            gemini: data?.accounts.filter((a) => a.provider === 'gemini').length ?? 0
          }}
          privacyMode={privacyMode}
          updateAvailable={Boolean(updateInfo?.updateAvailable)}
          updateVersion={updateInfo?.version}
          closeToTray={data?.appSettings.closeToTray}
          onSelectProvider={handleSelectProvider}
          onTogglePrivacyMode={togglePrivacyMode}
          onOpenSettings={() => setSettingsOpen(true)}
        />

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
