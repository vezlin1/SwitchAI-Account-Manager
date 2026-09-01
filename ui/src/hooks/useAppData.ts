import { useCallback, useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api, describeIpcError } from '../api'
import type {
  Account,
  AppData,
  AppSettings,
  AppStateChangedEvent,
  RecoveryStatus,
  StartupStatus
} from '../types'
import { isStaleRevision, mergeAccountSnapshot, mergeIncomingState } from '../utils/stateMerge'

export type AppDataUpdater = AppData | ((current: AppData) => AppData)

type StartupState = {
  status: StartupStatus | null
  recovery: RecoveryStatus | null
  warnings: string[]
  loading: boolean
}

type SettingsPatch = Partial<AppSettings>

function settingsPatch(previous: AppSettings, next: AppSettings): SettingsPatch {
  const patch: SettingsPatch = {}
  if (previous.autoRefreshEnabled !== next.autoRefreshEnabled) patch.autoRefreshEnabled = next.autoRefreshEnabled
  if (previous.autoRefreshIntervalMinutes !== next.autoRefreshIntervalMinutes) patch.autoRefreshIntervalMinutes = next.autoRefreshIntervalMinutes
  if (previous.closeToTray !== next.closeToTray) patch.closeToTray = next.closeToTray
  if (previous.skipUnsupportedRegionRefresh !== next.skipUnsupportedRegionRefresh) {
    patch.skipUnsupportedRegionRefresh = next.skipUnsupportedRegionRefresh
  }
  if (JSON.stringify(previous.hiddenSubscriptionCategories) !== JSON.stringify(next.hiddenSubscriptionCategories)) {
    patch.hiddenSubscriptionCategories = next.hiddenSubscriptionCategories
  }
  if (JSON.stringify(previous.hiddenAccountIds) !== JSON.stringify(next.hiddenAccountIds)) {
    patch.hiddenAccountIds = next.hiddenAccountIds
  }
  if (previous.lastActiveProvider !== next.lastActiveProvider) {
    patch.lastActiveProvider = next.lastActiveProvider
  }
  if (JSON.stringify(previous.geminiSwitchTargets) !== JSON.stringify(next.geminiSwitchTargets)) {
    patch.geminiSwitchTargets = next.geminiSwitchTargets
  }
  return patch
}

function applySettingsPatches(
  base: AppSettings,
  patches: Iterable<SettingsPatch>
): AppSettings {
  let result = base
  for (const patch of patches) result = { ...result, ...patch }
  return result
}

function nextState(
  current: AppData | null,
  next: AppData,
  settingsOverlay: AppSettings | null
): AppData {
  if (current == null) return next
  return mergeIncomingState(current, next, settingsOverlay)
}

export function useAppData() {
  const [data, setDataState] = useState<AppData | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [startup, setStartup] = useState<StartupState>({
    status: null,
    recovery: null,
    warnings: [],
    loading: true
  })
  const dataRef = useRef<AppData | null>(null)
  const reloadInFlightRef = useRef<Promise<void> | null>(null)
  const accountReloadsRef = useRef<Map<string, Promise<void>>>(new Map())
  const settingsQueueRef = useRef<Promise<AppData | null>>(Promise.resolve(null))
  const settingsVersionRef = useRef(0)
  const settingsPendingRef = useRef(0)
  const latestSettingsRef = useRef<AppSettings | null>(null)
  const authoritativeSettingsRef = useRef<AppSettings | null>(null)
  const pendingSettingsPatchesRef = useRef<Map<number, SettingsPatch>>(new Map())
  const settingsNeedsReconcileRef = useRef(false)
  const disposeRef = useRef(false)

  const setStartupState = useCallback((next: Partial<StartupState>) => {
    setStartup((previous) => {
      const resolved = { ...previous, ...next }
      return resolved
    })
  }, [])

  const setData = useCallback((next: AppDataUpdater): AppData | null => {
    const current = dataRef.current
    if (typeof next === 'function') {
      if (!current) return null
      const resolved = next(current)
      dataRef.current = resolved
      setDataState(resolved)
      return resolved
    }
    authoritativeSettingsRef.current = next.appSettings
    const resolved = nextState(
      current,
      next,
      settingsPendingRef.current > 0 || settingsNeedsReconcileRef.current
        ? latestSettingsRef.current
        : null
    )
    dataRef.current = resolved
    setDataState(resolved)
    return resolved
  }, [])

  const getData = useCallback(() => dataRef.current, [])

  const reconcileStartupState = useCallback((status: StartupStatus) => {
    setStartupState({
      status,
      recovery: status.recovery,
      warnings: status.warnings ?? []
    })
    if (status.mode === 'ready' && status.state) {
      setData(status.state)
      setLoading(false)
    }
  }, [setData, setStartupState])

  const restoreStateBackup = useCallback(async (): Promise<StartupStatus> => {
    try {
      const status = await api.restoreStateBackup()
      reconcileStartupState(status)
      return status
    } catch (err) {
      setError(describeIpcError(err))
      throw err
    }
  }, [reconcileStartupState])

  const startFresh = useCallback(async (): Promise<StartupStatus> => {
    try {
      const status = await api.startFresh()
      reconcileStartupState(status)
      return status
    } catch (err) {
      setError(describeIpcError(err))
      throw err
    }
  }, [reconcileStartupState])

  const openRecoveryDataDirectory = useCallback(async () => {
    try {
      await api.openRecoveryDataDirectory()
    } catch (err) {
      setError(describeIpcError(err))
      throw err
    }
  }, [])

  const saveAppSettings = useCallback((settings: AppSettings): Promise<AppData | null> => {
    const version = settingsVersionRef.current + 1
    settingsVersionRef.current = version
    const previous = latestSettingsRef.current
      ?? authoritativeSettingsRef.current
      ?? dataRef.current?.appSettings
      ?? settings
    const patch = settingsPatch(previous, settings)
    pendingSettingsPatchesRef.current.set(version, patch)
    settingsPendingRef.current = pendingSettingsPatchesRef.current.size
    latestSettingsRef.current = settings
    setData((current) => current ? { ...current, appSettings: settings } : current)

    const run = async (): Promise<AppData | null> => {
      try {
        const base = authoritativeSettingsRef.current
          ?? dataRef.current?.appSettings
          ?? settings
        const requestedSettings = { ...base, ...patch }
        const state = await api.setAppSettings(requestedSettings)
        authoritativeSettingsRef.current = state.appSettings
        pendingSettingsPatchesRef.current.delete(version)
        settingsPendingRef.current = pendingSettingsPatchesRef.current.size
        const overlay = settingsPendingRef.current > 0
          ? applySettingsPatches(state.appSettings, pendingSettingsPatchesRef.current.values())
          : null
        latestSettingsRef.current = overlay
        settingsNeedsReconcileRef.current = false
        const current = dataRef.current
        if (!current) {
          const resolved = overlay ? { ...state, appSettings: overlay } : state
          dataRef.current = resolved
          setDataState(resolved)
          return resolved
        }
        const resolved = mergeIncomingState(current, state, overlay)
        dataRef.current = resolved
        setDataState(resolved)
        return dataRef.current
      } catch (err) {
        setError(describeIpcError(err))
        pendingSettingsPatchesRef.current.delete(version)
        settingsPendingRef.current = pendingSettingsPatchesRef.current.size
        try {
          const authoritative = await api.getState()
          authoritativeSettingsRef.current = authoritative.appSettings
          settingsNeedsReconcileRef.current = false
          const overlay = settingsPendingRef.current > 0
            ? applySettingsPatches(
                authoritative.appSettings,
                pendingSettingsPatchesRef.current.values()
              )
            : null
          latestSettingsRef.current = overlay
          const current = dataRef.current
          if (current) {
            const resolved = mergeIncomingState(current, authoritative, overlay)
            dataRef.current = resolved
            setDataState(resolved)
          }
        } catch {
          settingsNeedsReconcileRef.current = true
          const base = authoritativeSettingsRef.current
            ?? dataRef.current?.appSettings
            ?? settings
          const overlay = settingsPendingRef.current > 0
            ? applySettingsPatches(base, pendingSettingsPatchesRef.current.values())
            : null
          latestSettingsRef.current = overlay
          if (overlay) {
            const current = dataRef.current
            if (current) {
              const resolved = { ...current, appSettings: overlay }
              dataRef.current = resolved
              setDataState(resolved)
            }
          }
        }
        throw err
      }
    }

    const task = settingsQueueRef.current.then(run, run)
    settingsQueueRef.current = task.catch(() => dataRef.current)
    return task
  }, [setData])

  const clearError = useCallback(() => setError(null), [])

  const reload = useCallback(async () => {
    if (reloadInFlightRef.current) {
      return reloadInFlightRef.current
    }

    const initialLoad = dataRef.current == null
    const request = (async () => {
      if (initialLoad) setLoading(true)
      setError(null)
      try {
        const state = await api.getState()
        if (!disposeRef.current) {
          setData(state)
          setLoading(false)
        }
      } catch (err) {
        if (!disposeRef.current) setError(describeIpcError(err))
      } finally {
        if (initialLoad) setLoading(false)
        reloadInFlightRef.current = null
      }
    })()

    reloadInFlightRef.current = request
    return request
  }, [setData])

  const reloadAccountOrder = useCallback(async (shouldApply: () => boolean) => {
    try {
      const state = await api.getState()
      if (!shouldApply()) return
      setData((latest) => {
        if (!shouldApply()) return latest
        if (isStaleRevision(state.revision, latest.revision)) return latest
        const latestById = new Map(latest.accounts.map((account) => [account.id, account]))
        const serverIds = new Set(state.accounts.map((account) => account.id))
        const ordered = state.accounts.flatMap((account) => {
          const current = latestById.get(account.id)
          return current ? [mergeAccountSnapshot(current, account)] : []
        })
        const remaining = latest.accounts.filter((account) => !serverIds.has(account.id))
        return { ...latest, accounts: [...ordered, ...remaining], revision: state.revision }
      })
    } catch (err) {
      setError(describeIpcError(err))
    }
  }, [setData])

  const reloadAccount = useCallback((accountId: string) => {
    const existing = accountReloadsRef.current.get(accountId)
    if (existing) return existing

    const request = (async () => {
      try {
        const snapshot = await api.getAccount(accountId)
        if (disposeRef.current) return
        const resolved = setData((latest) => {
          if (!latest.accounts.some((candidate) => candidate.id === snapshot.account.id)) {
            return latest
          }
          if (isStaleRevision(snapshot.revision, latest.revision)) {
            return latest
          }
          return {
            ...latest,
            accounts: latest.accounts.map((candidate) =>
              candidate.id === snapshot.account.id
                ? mergeAccountSnapshot(candidate, snapshot.account)
                : candidate
            )
          }
        })
        if (!resolved || !resolved.accounts.some((candidate) => candidate.id === snapshot.account.id)) {
          await reload()
        }
      } catch {
        if (!disposeRef.current) await reload()
      } finally {
        accountReloadsRef.current.delete(accountId)
      }
    })()
    accountReloadsRef.current.set(accountId, request)
    return request
  }, [reload, setData])

  useEffect(() => {
    disposeRef.current = false
    return () => {
      disposeRef.current = true
    }
  }, [])

  useEffect(() => {
    let disposed = false
    let unlisten: UnlistenFn | undefined
    let unlistenAccount: UnlistenFn | undefined
    let fallbackTimer: number | undefined

    void listen<Account>('account-updated', ({ payload }) => {
      if (disposeRef.current) return
      setData((latest) => {
        if (!latest) return latest
        const index = latest.accounts.findIndex((a) => a.id === payload.id)
        if (index === -1) return latest
        const accounts = [...latest.accounts]
        accounts[index] = mergeAccountSnapshot(accounts[index], payload)
        return { ...latest, accounts }
      })
    }).then((dispose) => {
      if (disposed) dispose()
      else unlistenAccount = dispose
    })

    void listen<AppStateChangedEvent>('app-state-changed', ({ payload }) => {
      if (payload.scope === 'account' && payload.accountIds.length === 1) {
        void reloadAccount(payload.accountIds[0])
        return
      }
      void reload()
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch(() => {
      if (!disposed) {
        fallbackTimer = window.setInterval(() => {
          void reload()
        }, 30000)
      }
    })

    return () => {
      disposed = true
      unlisten?.()
      unlistenAccount?.()
      if (fallbackTimer !== undefined) window.clearInterval(fallbackTimer)
    }
  }, [reload, reloadAccount, setData])

  useEffect(() => {
    void api.getStartupStatus()
      .then((status) => {
        if (disposeRef.current) return
        reconcileStartupState(status)
        setStartupState({ loading: false })
      })
      .catch((err) => {
        if (disposeRef.current) return
        setError(describeIpcError(err))
        setStartupState({ loading: false })
      })
  }, [reconcileStartupState, setStartupState])

  return {
    data,
    setData,
    getData,
    saveAppSettings,
    loading,
    error,
    clearError,
    reload,
    reloadAccountOrder,
    startup: startup.status,
    recovery: startup.recovery,
    startupWarnings: startup.warnings,
    startupLoading: startup.loading,
    restoreStateBackup,
    startFresh,
    openRecoveryDataDirectory
  }
}
