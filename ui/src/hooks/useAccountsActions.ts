import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, describeIpcError } from '../api'
import type { Account, AccountProvider, AppData, AppSettings, CommandWarning } from '../types'
import type { AppDataUpdater } from './useAppData'
import {
  addBusyCount,
  keyIsBusy,
  type BusyCounters,
  type BusyKey
} from '../utils/busy'

type UseAccountsActionsArgs = {
  setData: (next: AppDataUpdater) => AppData | null
  getData?: () => AppData | null
  persistAppSettings: (settings: AppSettings) => Promise<AppData | null>
  onError?: (message: string | null) => void
  confirmSwitch?: (account: Account) => Promise<boolean>
}

function warningMessage(
  warning: CommandWarning,
  fallback: string
): string {
  if (warning.message.trim()) return warning.message
  const suffix = warning.accountId ? ` (${warning.accountId})` : ''
  return `${fallback}${suffix}`
}

export function useAccountsActions({
  setData,
  getData,
  persistAppSettings,
  onError,
  confirmSwitch
}: UseAccountsActionsArgs) {
  const [busyCounters, setBusyCounters] = useState<BusyCounters>({})
  const [refreshingAll, setRefreshingAll] = useState(false)
  const [errors, setErrors] = useState<Record<AccountProvider, string | null>>({
    codex: null,
    gemini: null
  })
  const refreshAllInFlightRef = useRef(false)
  const orderQueueRef = useRef<Promise<void>>(Promise.resolve())
  const orderVersionRef = useRef(0)
  const onErrorRef = useRef(onError)
  const confirmSwitchRef = useRef(confirmSwitch)

  useEffect(() => {
    onErrorRef.current = onError
  }, [onError])

  useEffect(() => {
    confirmSwitchRef.current = confirmSwitch
  }, [confirmSwitch])

  const getAccountProvider = useCallback((accountId?: string | null): AccountProvider => {
    if (accountId && getData) {
      const data = getData()
      const candidate = data?.accounts.find((a) => a.id === accountId)
      if (candidate?.provider) return candidate.provider
    }
    return 'codex'
  }, [getData])

  const busyKeys = useMemo<ReadonlySet<string>>(
    () => new Set(Object.keys(busyCounters)),
    [busyCounters]
  )
  const beginOp = useCallback((key: BusyKey) => {
    setBusyCounters((prev) => addBusyCount(prev, key, 1))
  }, [])
  const endOp = useCallback((key: BusyKey) => {
    setBusyCounters((prev) => addBusyCount(prev, key, -1))
  }, [])

  const setProviderError = useCallback((provider: AccountProvider, message: string | null) => {
    setErrors((prev) => ({ ...prev, [provider]: message }))
  }, [])

  const clearError = useCallback((provider?: AccountProvider) => {
    if (provider) {
      setErrors((prev) => ({ ...prev, [provider]: null }))
    } else {
      setErrors({ codex: null, gemini: null })
    }
  }, [])

  const getError = useCallback((provider: AccountProvider) => {
    return errors[provider]
  }, [errors])

  const reportWarning = useCallback((message: string | null, provider: AccountProvider = 'codex') => {
    if (!message) return
    const lower = message.toLowerCase()
    const targetProv: AccountProvider =
      lower.includes('gemini') || lower.includes('antigravity') || lower.includes('google')
        ? 'gemini'
        : lower.includes('chatgpt') || lower.includes('codex') || lower.includes('openai')
          ? 'codex'
          : provider
    setProviderError(targetProv, message)
    if (onErrorRef.current) onErrorRef.current(message)
  }, [setProviderError])

  const reportWarnings = useCallback((warnings: CommandWarning[], defaultProvider: AccountProvider = 'codex') => {
    for (const warning of warnings) {
      reportWarning(warningMessage(warning, 'Operation completed with a warning'), defaultProvider)
    }
  }, [reportWarning])

  const removeAccount = useCallback(async (accountId: string) => {
    const provider = getAccountProvider(accountId)
    const key: BusyKey = `delete:${accountId}`
    beginOp(key)
    try {
      clearError(provider)
      const next = await api.removeAccount(accountId)
      setData(next)
      setData((current) => ({
        ...current,
        appSettings: {
          ...current.appSettings,
          hiddenAccountIds: current.appSettings.hiddenAccountIds.filter((id) => id !== accountId)
        }
      }))
    } catch (err) {
      setProviderError(provider, describeIpcError(err))
    } finally {
      endOp(key)
    }
  }, [beginOp, endOp, setData, clearError, setProviderError, getAccountProvider])

  const switchAccount = useCallback(async (account: Account) => {
    const confirmed = confirmSwitchRef.current
      ? await confirmSwitchRef.current(account)
      : false
    if (!confirmed) return

    const isGemini = account.provider === 'gemini'
    const provider: AccountProvider = isGemini ? 'gemini' : 'codex'
    const key: BusyKey = `switch:${account.id}`
    beginOp(key)
    try {
      setProviderError(provider, null)
      const response = isGemini
        ? await api.switchActiveAccountAndRestartAntigravity(account.id)
        : await api.switchActiveAccountAndRestartCodex(account.id)
      setData(response.state)
      if (response.restartWarning) {
        const appName = isGemini ? 'Antigravity' : 'ChatGPT'
        setProviderError(provider, `Account switched, but ${appName} restart failed: ${response.restartWarning}`)
      }
    } catch (err) {
      setProviderError(provider, describeIpcError(err))
    } finally {
      endOp(key)
    }
  }, [beginOp, endOp, setData, setProviderError])

  const saveOrder = useCallback((nextAccounts: Account[], provider: AccountProvider = 'codex') => {
    const version = orderVersionRef.current + 1
    orderVersionRef.current = version
    const accountIds = nextAccounts.map((account) => account.id)
    const run = async () => {
      beginOp('order')
      try {
        clearError(provider)
        const next = await api.setAccountOrder(accountIds)
        if (orderVersionRef.current === version) {
          setData((latest) => {
            if (next.revision < latest.revision) return latest
            const latestById = new Map(latest.accounts.map((account) => [account.id, account]))
            const serverIds = new Set(next.accounts.map((account) => account.id))
            const ordered = next.accounts.flatMap((account) => {
              const candidate = latestById.get(account.id)
              return candidate ? [candidate] : []
            })
            const remaining = latest.accounts.filter((account) => !serverIds.has(account.id))
            return { ...latest, accounts: [...ordered, ...remaining], revision: next.revision }
          })
        }
      } catch (err) {
        setProviderError(provider, describeIpcError(err))
        throw err
      } finally {
        endOp('order')
      }
    }
    const task = orderQueueRef.current.then(run, run)
    orderQueueRef.current = task.catch(() => undefined)
    return task
  }, [beginOp, endOp, setData, clearError, setProviderError])

  const detectSubscription = useCallback(async (accountId: string) => {
    const provider = getAccountProvider(accountId)
    const key: BusyKey = `subscription-detect:${accountId}`
    beginOp(key)
    try {
      setProviderError(provider, null)
      const response = await api.refreshAccountSubscription(accountId)
      setData(response.state)
      reportWarnings(response.warnings, provider)
    } catch (err) {
      setProviderError(provider, describeIpcError(err))
    } finally {
      endOp(key)
    }
  }, [beginOp, endOp, reportWarnings, setData, setProviderError, getAccountProvider])

  const refreshAccount = useCallback(async (accountId: string) => {
    const provider = getAccountProvider(accountId)
    const key: BusyKey = `quota:${accountId}`
    beginOp(key)
    try {
      setProviderError(provider, null)
      const response = await api.refreshAccountQuota(accountId)
      setData(response.state)
      reportWarnings(response.warnings, provider)
    } catch (err) {
      setProviderError(provider, describeIpcError(err))
    } finally {
      endOp(key)
    }
  }, [beginOp, endOp, reportWarnings, setData, setProviderError, getAccountProvider])

  const saveAppSettings = useCallback(async (settings: AppSettings, provider: AccountProvider = 'codex') => {
    const key: BusyKey = 'settings:subscription-visibility'
    beginOp(key)
    try {
      clearError(provider)
      await persistAppSettings(settings)
    } catch (err) {
      setProviderError(provider, describeIpcError(err))
    } finally {
      endOp(key)
    }
  }, [beginOp, endOp, persistAppSettings, clearError, setProviderError])

  const refreshAll = useCallback(async (provider?: AccountProvider) => {
    if (refreshAllInFlightRef.current) return
    refreshAllInFlightRef.current = true
    const currentProv: AccountProvider = provider ?? 'codex'
    beginOp('refresh-all')
    try {
      setRefreshingAll(true)
      setProviderError(currentProv, null)
      const response = await api.refreshAllQuotas(provider)
      setData(response.state)
      reportWarnings(response.warnings, currentProv)
    } catch (err) {
      setProviderError(currentProv, describeIpcError(err))
    } finally {
      setRefreshingAll(false)
      refreshAllInFlightRef.current = false
      endOp('refresh-all')
    }
  }, [beginOp, endOp, reportWarnings, setData, setProviderError])

  const importAntigravity = useCallback(async () => {
    const key: BusyKey = 'import:antigravity'
    beginOp(key)
    try {
      setProviderError('gemini', null)
      const response = await api.importAntigravityAccount()
      setData(response.state)
      reportWarnings(response.warnings, 'gemini')
    } catch (err) {
      setProviderError('gemini', describeIpcError(err))
    } finally {
      endOp(key)
    }
  }, [beginOp, endOp, reportWarnings, setData, setProviderError])

  return {
    busyKeys,
    refreshingAll,
    errors,
    error: errors.codex ?? errors.gemini,
    getError,
    clearError,
    isBusy: (key: BusyKey) => keyIsBusy(busyCounters, key),
    busyCount: (key: BusyKey) => busyCounters[key] ?? 0,
    removeAccount,
    switchAccount,
    saveOrder,
    detectSubscription,
    refreshAccount,
    saveAppSettings,
    refreshAll,
    importAntigravity
  }
}
