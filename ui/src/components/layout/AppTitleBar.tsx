import { memo, useEffect, useState } from 'react'
import { Eye, EyeOff, Maximize2, Minimize2, Minus, Settings, X } from 'lucide-react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import appLogo from '../../assets/app-icon.png'
import type { AccountProvider } from '../../types'

const appWindow = getCurrentWindow()

export type AppTitleBarProps = {
  isMac: boolean
  appVersion: string | null
  enabledProviders: AccountProvider[]
  activeProvider: AccountProvider
  accountCounts: { codex: number; gemini: number }
  privacyMode: boolean
  updateAvailable: boolean
  updateVersion?: string
  closeToTray?: boolean
  onSelectProvider: (provider: AccountProvider) => void
  onTogglePrivacyMode: () => void
  onOpenSettings: () => void
}

export const AppTitleBar = memo(function AppTitleBar({
  isMac,
  appVersion,
  enabledProviders,
  activeProvider,
  accountCounts,
  privacyMode,
  updateAvailable,
  updateVersion,
  closeToTray = false,
  onSelectProvider,
  onTogglePrivacyMode,
  onOpenSettings
}: AppTitleBarProps) {
  const [isMaximized, setIsMaximized] = useState(false)

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

  return (
    <header
      className="app-header w-full flex items-center justify-between sticky top-0 z-30 select-none"
      data-tauri-drag-region
      onDoubleClick={(e) => {
        const target = e.target as HTMLElement
        if (
          target === e.currentTarget ||
          target.hasAttribute('data-tauri-drag-region') ||
          target.classList.contains('app-header') ||
          target.classList.contains('app-title')
        ) {
          void toggleMaximizeWindow()
        }
      }}
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
        {enabledProviders.length > 1 ? (
          <div className="provider-tabs-nav" role="tablist" aria-label="Provider selection" data-no-drag>
            {enabledProviders.includes('codex') && (
              <button
                type="button"
                role="tab"
                aria-selected={activeProvider === 'codex'}
                className={`provider-tab-pill ${activeProvider === 'codex' ? 'provider-tab-pill-active' : ''}`}
                onClick={() => onSelectProvider('codex')}
              >
                <span className="provider-tab-title">ChatGPT</span>
                <span className="provider-tab-count">
                  {accountCounts.codex}
                </span>
              </button>
            )}
            {enabledProviders.includes('gemini') && (
              <button
                type="button"
                role="tab"
                aria-selected={activeProvider === 'gemini'}
                className={`provider-tab-pill ${activeProvider === 'gemini' ? 'provider-tab-pill-active' : ''}`}
                onClick={() => onSelectProvider('gemini')}
              >
                <span className="provider-tab-title">Gemini</span>
                <span className="provider-tab-count">
                  {accountCounts.gemini}
                </span>
              </button>
            )}
          </div>
        ) : enabledProviders.length === 1 ? (
          <div className="provider-tabs-nav" data-no-drag>
            <div className="provider-tab-pill provider-tab-pill-active cursor-default">
              <span className="provider-tab-title">
                {enabledProviders[0] === 'codex' ? 'ChatGPT' : 'Gemini'}
              </span>
              <span className="provider-tab-count">
                {enabledProviders[0] === 'codex' ? accountCounts.codex : accountCounts.gemini}
              </span>
            </div>
          </div>
        ) : null}
      </div>

      {/* Right: Actions / Controls */}
      <div className="flex items-center justify-end h-full shrink-0" data-no-drag>
        <button
          type="button"
          className={`header-icon-btn ${privacyMode ? 'header-icon-btn-active' : ''} mr-1`}
          onClick={onTogglePrivacyMode}
          title={
            privacyMode
              ? isMac
                ? 'Privacy Mode: ON (Click to reveal sensitive data · ⇧⌘P)'
                : 'Privacy Mode: ON (Click to reveal sensitive data · Ctrl+Shift+P)'
              : isMac
                ? 'Privacy Mode: OFF (Click to hide sensitive data · ⇧⌘P)'
                : 'Privacy Mode: OFF (Click to hide sensitive data · Ctrl+Shift+P)'
          }
          aria-label={privacyMode ? 'Disable privacy mode' : 'Enable privacy mode'}
          aria-pressed={privacyMode}
        >
          {privacyMode ? <EyeOff size={14} /> : <Eye size={14} />}
        </button>

        <button
          className={`header-icon-btn ${isMac ? 'mr-3' : 'mr-1'} relative`}
          onClick={onOpenSettings}
          title={
            updateAvailable
              ? `Settings (Update available v${updateVersion ?? ''})`
              : isMac
                ? 'Settings (⌘,)'
                : 'Settings (Ctrl+,)'
          }
          aria-label="Settings"
        >
          <Settings size={14} />
          {updateAvailable && (
            <span
              className="absolute top-1 right-1 w-2 h-2 rounded-full bg-blue-500 shadow-[0_0_8px_rgba(59,130,246,0.9)] animate-pulse"
              aria-hidden="true"
            />
          )}
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
              title={closeToTray ? 'Hide to tray' : 'Close'}
              aria-label={closeToTray ? 'Hide to tray' : 'Close'}
            >
              <X size={14} strokeWidth={2} />
            </button>
          </div>
        )}
      </div>
    </header>
  )
})
