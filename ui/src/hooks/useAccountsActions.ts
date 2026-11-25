import { useCallback, useMemo, useRef, useState } from 'react'
import { api, describeIpcError } from '../api'
import type {
  Account, AccountProvider, AccountRefreshResult, AppData, AppSettings, CommandWarning, StateResult
} from '../types'
import type { AppDataUpdater } from './useAppData'
import {
  addBusyCount,
  type BusyCounters,
  type BusyKey
} from '../utils/busy'
import { applyServerAccountOrder, mergeAccountUpdates } from '../utils/stateMerge'

type UseAccountsActionsArgs = {
  setData: (next: AppDataUpdater) => AppData | null
  getData: () => AppData | null
  persistAppSettings: (settings: AppSettings) => Promise<AppData | null>
  confirmSwitch: (account: Account) => Promise<boolean>
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
  confirmSwitch
}: UseAccountsActionsArgs) {
  const [busyCounters, setBusyCounters] = useState<BusyCounters>({})
  const [errors, setErrors] = useState<Record<AccountProvider, string | null>>({
    codex: null,
    gemini: null
  })
  const refreshAllInFlightRef = useRef(false)
  const orderQueueRef = useRef<Promise<void>>(Promise.resolve())
  const orderVersionRef = useRef(0)

  const getAccountProvider = useCallback((accountId: string): AccountProvider => (
    getData()?.accounts.find((account) => account.id === accountId)?.provider ?? 'codex'
  ), [getData])

  const busyKeys = useMemo<ReadonlySet<string>>(
    () => new Set(Object.keys(busyCounters)),
    [busyCounters]
  )

  const setProviderError = useCallback((provider: AccountProvider, message: string | null) => {
    setErrors((prev) => ({ ...prev, [provider]: message }))
  }, [])

  const clearError = useCallback((provider: AccountProvider) => {
    setProviderError(provider, null)
  }, [setProviderError])

  const getError = useCallback((provider: AccountProvider) => errors[provider], [errors])

  const runOperation = useCallback(async (
    key: BusyKey,
    provider: AccountProvider,
    operation: () => Promise<void>,
    options?: { rethrow: boolean }
  ) => {
    setBusyCounters((previous) => addBusyCount(previous, key, 1))
    try {
      clearError(provider)
      await operation()
    } catch (err) {
      setProviderError(provider, describeIpcError(err))
      if (options?.rethrow) throw err
    } finally {
      setBusyCounters((previous) => addBusyCount(previous, key, -1))
    }
  }, [clearError, setProviderError])

  const reportWarnings = useCallback((warnings: CommandWarning[], provider: AccountProvider) => {
    if (!warnings?.length) return
    const message = warnings.map((warning) => warningMessage(warning, 'Operation completed with a warning')).join(' · ')
    const lower = message.toLowerCase()
    const targetProvider: AccountProvider =
      lower.includes('gemini') || lower.includes('antigravity') || lower.includes('google')
        ? 'gemini'
        : lower.includes('chatgpt') || lower.includes('codex') || lower.includes('openai')
          ? 'codex'
          : provider
    setProviderError(targetProvider, message)
  }, [setProviderError])

  const applyStateResult = useCallback((result: StateResult, provider: AccountProvider) => {
    setData(result.state)
    reportWarnings(result.warnings, provider)
  }, [reportWarnings, setData])

  const removeAccount = useCallback((accountId: string) => (
    runOperation(`delete:${accountId}`, getAccountProvider(accountId), async () => {
      setData(await api.removeAccount(accountId))
    })
  ), [getAccountProvider, runOperation, setData])

  const switchAccount = useCallback(async (account: Account) => {
    const confirmed = await confirmSwitch(account)
    if (!confirmed) return

    const isGemini = account.provider === 'gemini'
    const provider = isGemini ? 'gemini' : 'codex'
    await runOperation(`switch:${account.id}`, provider, async () => {
      const response = isGemini
        ? await api.switchActiveAccountAndRestartAntigravity(account.id)
        : await api.switchActiveAccountAndRestartCodex(account.id)
      setData(response.state)
      if (response.restartWarning) {
        const appName = isGemini ? 'Antigravity' : 'ChatGPT'
        setProviderError(provider, `Account switched, but ${appName} restart failed: ${response.restartWarning}`)
      }
    })
  }, [confirmSwitch, runOperation, setData, setProviderError])

  const saveOrder = useCallback((nextAccounts: Account[], provider: AccountProvider = 'codex') => {
    const version = ++orderVersionRef.current
    const accountIds = nextAccounts.map((account) => account.id)
    const run = () => runOperation('order', provider, async () => {
      const next = await api.setAccountOrder(accountIds)
      if (orderVersionRef.current === version) {
        setData((latest) => applyServerAccountOrder(latest, next))
      }
    }, { rethrow: true })
    const task = orderQueueRef.current.then(run)
    orderQueueRef.current = task.catch(() => undefined)
    return task
  }, [runOperation, setData])

  const refreshAccountData = useCallback((
    accountId: string,
    key: BusyKey,
    request: (id: string) => Promise<AccountRefreshResult>
  ) => {
    const provider = getAccountProvider(accountId)
    return runOperation(key, provider, async () => {
      const response = await request(accountId)
      setData((latest) => mergeAccountUpdates(latest, [response.account]))
      reportWarnings(response.warnings, provider)
    })
  }, [getAccountProvider, reportWarnings, runOperation, setData])

  const detectSubscription = useCallback((accountId: string) => (
    refreshAccountData(accountId, `subscription-detect:${accountId}`, api.refreshAccountSubscription)
  ), [refreshAccountData])

  const refreshAccount = useCallback((accountId: string) => (
    refreshAccountData(accountId, `quota:${accountId}`, api.refreshAccountQuota)
  ), [refreshAccountData])

  const saveAppSettings = useCallback((settings: AppSettings, provider: AccountProvider = 'codex') => (
    runOperation('settings:subscription-visibility', provider, async () => {
      await persistAppSettings(settings)
    })
  ), [persistAppSettings, runOperation])

  const refreshAll = useCallback(async (provider?: AccountProvider) => {
    if (refreshAllInFlightRef.current) return
    refreshAllInFlightRef.current = true
    const currentProvider = provider ?? 'codex'
    try {
      await runOperation('refresh-all', currentProvider, async () => {
        applyStateResult(await api.refreshAllQuotas(provider), currentProvider)
      })
    } finally {
      refreshAllInFlightRef.current = false
    }
  }, [applyStateResult, runOperation])

  const importSession = useCallback((provider: AccountProvider) => (
    runOperation(`import:${provider}`, provider, async () => {
      const result = provider === 'gemini'
        ? await api.importAntigravityAccount()
        : await api.importCodexAccount()
      applyStateResult(result, provider)
    })
  ), [applyStateResult, runOperation])

  return {
    busyKeys,
    refreshingAll: busyKeys.has('refresh-all'),
    getError,
    clearError,
    removeAccount,
    switchAccount,
    saveOrder,
    detectSubscription,
    refreshAccount,
    saveAppSettings,
    refreshAll,
    importSession
  }
}
