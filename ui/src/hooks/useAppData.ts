import { settingsPatch, type SettingsPatch } from '../utils/settingsPatch'
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
import {
  applyServerAccountOrder, isStaleRevision, mergeAccountSnapshot, mergeAccountUpdates, mergeIncomingState
} from '../utils/stateMerge'

export type AppDataUpdater = AppData | ((current: AppData) => AppData)

type StartupState = {
  status: StartupStatus | null
  recovery: RecoveryStatus | null
  warnings: string[]
  loading: boolean
}

function pendingSettingsOverlay(
  base: AppSettings,
  patches: ReadonlyMap<number, SettingsPatch>
): AppSettings | null {
  if (patches.size === 0) return null
  const overlay = { ...base }
  for (const patch of patches.values()) Object.assign(overlay, patch)
  return overlay
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

  const publishData = useCallback((next: AppData): AppData => {
    dataRef.current = next
    setDataState(next)
    return next
  }, [])

  const setData = useCallback((next: AppDataUpdater): AppData | null => {
    const current = dataRef.current
    if (typeof next === 'function') {
      if (!current) return null
      return publishData(next(current))
    }
    authoritativeSettingsRef.current = next.appSettings
    return publishData(nextState(
      current,
      next,
      pendingSettingsPatchesRef.current.size > 0 || settingsNeedsReconcileRef.current
        ? latestSettingsRef.current
        : null
    ))
  }, [publishData])

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

  const runRecoveryCommand = useCallback(async (command: () => Promise<StartupStatus>) => {
    try {
      const status = await command()
      reconcileStartupState(status)
      return status
    } catch (err) {
      setError(describeIpcError(err))
      throw err
    }
  }, [reconcileStartupState])

  const restoreStateBackup = useCallback(() => runRecoveryCommand(api.restoreStateBackup), [runRecoveryCommand])
  const startFresh = useCallback(() => runRecoveryCommand(api.startFresh), [runRecoveryCommand])

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
        const overlay = pendingSettingsOverlay(state.appSettings, pendingSettingsPatchesRef.current)
        latestSettingsRef.current = overlay
        settingsNeedsReconcileRef.current = false
        const current = dataRef.current
        if (!current) {
          return publishData(overlay ? { ...state, appSettings: overlay } : state)
        }
        return publishData(mergeIncomingState(current, state, overlay))
      } catch (err) {
        setError(describeIpcError(err))
        pendingSettingsPatchesRef.current.delete(version)
        try {
          const authoritative = await api.getState()
          authoritativeSettingsRef.current = authoritative.appSettings
          settingsNeedsReconcileRef.current = false
          const overlay = pendingSettingsOverlay(authoritative.appSettings, pendingSettingsPatchesRef.current)
          latestSettingsRef.current = overlay
          const current = dataRef.current
          if (current) {
            publishData(mergeIncomingState(current, authoritative, overlay))
          }
        } catch {
          settingsNeedsReconcileRef.current = true
          const base = authoritativeSettingsRef.current
            ?? dataRef.current?.appSettings
            ?? settings
          const overlay = pendingSettingsOverlay(base, pendingSettingsPatchesRef.current)
          latestSettingsRef.current = overlay
          const current = dataRef.current
          if (overlay && current) {
            publishData({ ...current, appSettings: overlay })
          }
        }
        throw err
      }
    }

    const task = settingsQueueRef.current.then(run, run)
    settingsQueueRef.current = task.catch(() => dataRef.current)
    return task
  }, [publishData, setData])

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
        return applyServerAccountOrder(latest, state, mergeAccountSnapshot)
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
          return mergeAccountUpdates(latest, [snapshot.account])
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
    let accountBatchTimer: number | undefined
    const pendingAccounts: Account[] = []

    const flushAccountUpdates = () => {
      if (accountBatchTimer !== undefined) window.clearTimeout(accountBatchTimer)
      accountBatchTimer = undefined
      if (disposed || disposeRef.current || pendingAccounts.length === 0) return
      const updates = pendingAccounts.splice(0)
      setData((current) => mergeAccountUpdates(current, updates))
    }

    void listen<{ accounts: Account[] }>('accounts-updated', ({ payload }) => {
      if (disposed || disposeRef.current || !dataRef.current) return
      for (const account of payload.accounts) pendingAccounts.push(account)
      if (accountBatchTimer === undefined) {
        accountBatchTimer = window.setTimeout(flushAccountUpdates, 32)
      }
    }).then((dispose) => {
      if (disposed) dispose()
      else unlistenAccount = dispose
    }).catch(() => undefined)

    void listen<AppStateChangedEvent>('app-state-changed', ({ payload }) => {
      if (disposed || disposeRef.current) return
      if (payload.state) {
        flushAccountUpdates()
        setData(payload.state)
        return
      }
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
      if (accountBatchTimer !== undefined) window.clearTimeout(accountBatchTimer)
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
