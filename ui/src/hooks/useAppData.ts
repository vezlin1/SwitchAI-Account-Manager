import { createRefreshQueue } from '../utils/refreshQueue'
import { settingsPatch, type SettingsPatch } from '../utils/settingsPatch'
import { useCallback, useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api, describeIpcError } from '../api'
import type {
  AccountSnapshot,
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
  const localAuthRequestedRef = useRef(false)
  const reloadQueueRef = useRef<ReturnType<typeof createRefreshQueue> | null>(null)
  const accountReloadsRef = useRef<Map<string, ReturnType<typeof createRefreshQueue>>>(new Map())
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

  const runReload = useCallback(async () => {
    if (disposeRef.current) return
    const initialLoad = dataRef.current == null
    const reconcileLocalAuth = localAuthRequestedRef.current
    localAuthRequestedRef.current = false
    if (initialLoad) setLoading(true)
    setError(null)
    try {
      const state = await api.getState(reconcileLocalAuth)
      if (!disposeRef.current) setData(state)
    } catch (err) {
      if (!disposeRef.current) setError(describeIpcError(err))
    } finally {
      if (!disposeRef.current) setLoading(false)
    }
  }, [setData])

  const reload = useCallback((reconcileLocalAuth = true) => {
    localAuthRequestedRef.current ||= reconcileLocalAuth
    reloadQueueRef.current ??= createRefreshQueue(runReload)
    return reloadQueueRef.current()
  }, [runReload])

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
    if (existing) return existing()

    const refresh = createRefreshQueue(async () => {
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
          let changed = false
          const accounts = latest.accounts.map((candidate) => {
            if (candidate.id !== snapshot.account.id) return candidate
            const merged = mergeAccountSnapshot(candidate, snapshot.account)
            changed ||= merged !== candidate
            return merged
          })
          return changed ? { ...latest, accounts } : latest
        })
        if (!resolved || !resolved.accounts.some((candidate) => candidate.id === snapshot.account.id)) {
          await reload(false)
        }
      } catch {
        if (!disposeRef.current) await reload(false)
      }
    })
    accountReloadsRef.current.set(accountId, refresh)
    return refresh().finally(() => {
      if (accountReloadsRef.current.get(accountId) === refresh) accountReloadsRef.current.delete(accountId)
    })
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

    let batchTimer: ReturnType<typeof setTimeout> | undefined
    let fullReload = false
    const accountIds = new Set<string>()
    const snapshots = new Map<string, AccountSnapshot>()
    const flush = () => {
      batchTimer = undefined
      if (disposed || disposeRef.current) return
      if (snapshots.size > 0) {
        setData((latest) => {
          let changed = false
          const accounts = latest.accounts.map((account) => {
            const snapshot = snapshots.get(account.id)
            if (!snapshot || isStaleRevision(snapshot.revision, latest.revision)) return account
            const merged = mergeAccountSnapshot(account, snapshot.account)
            changed ||= merged !== account
            return merged
          })
          return changed ? { ...latest, accounts } : latest
        })
        snapshots.clear()
      }
      if (fullReload) void reload(false)
      else for (const id of accountIds) void reloadAccount(id)
      fullReload = false
      accountIds.clear()
    }
    const scheduleFlush = () => {
      batchTimer ??= setTimeout(flush, 80)
    }

    void listen<AccountSnapshot>('account-updated', ({ payload }) => {
      if (disposed || disposeRef.current) return
      const previous = snapshots.get(payload.account.id)
      if (!previous || payload.revision >= previous.revision) snapshots.set(payload.account.id, payload)
      scheduleFlush()
    }).then((dispose) => {
      if (disposed) dispose()
      else unlistenAccount = dispose
    })

    void listen<AppStateChangedEvent>('app-state-changed', ({ payload }) => {
      if (disposed || disposeRef.current) return
      if (payload.scope === 'account' && payload.accountIds.length === 1) accountIds.add(payload.accountIds[0])
      else fullReload = true
      scheduleFlush()
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch(() => {
      if (!disposed) {
        fallbackTimer = window.setInterval(() => {
          void reload(false)
        }, 30000)
      }
    })

    return () => {
      disposed = true
      unlisten?.()
      unlistenAccount?.()
      if (batchTimer !== undefined) clearTimeout(batchTimer)
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
