import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, describeIpcError } from '../api'
import type { Account, AccountProvider, AccountRefreshResult, AppData, AppSettings, CommandWarning } from '../types'
import type { AppDataUpdater } from './useAppData'
import {
  addBusyCount,
  keyIsBusy,
  type BusyCounters,
  type BusyKey
} from '../utils/busy'
import { applyServerAccountOrder, mergeAccountUpdates } from '../utils/stateMerge'
import { splitRefreshWarnings, type AccountRefreshNotice } from '../utils/accountFeedback'

type UseAccountsActionsArgs = {
  setData: (next: AppDataUpdater) => AppData | null
  getData: () => AppData | null
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
  const [refreshingProviders, setRefreshingProviders] = useState<ReadonlySet<AccountProvider>>(new Set())
  const [errors, setErrors] = useState<Record<AccountProvider, string | null>>({
    codex: null,
    gemini: null
  })

  const [refreshNotices, setRefreshNotices] = useState(
    () => new Map<string, { account: Account; notice: AccountRefreshNotice }>()
  )
  const refreshAllInFlightRef = useRef(new Set<AccountProvider>())
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
    if (!warnings || warnings.length === 0) return
    const messages = warnings.map((w) => warningMessage(w, 'Operation completed with a warning')).filter(Boolean)
    if (messages.length > 0) {
      reportWarning(messages.join(' · '), defaultProvider)
    }
  }, [reportWarning])

  const applyRefreshNotices = useCallback((
    warnings: readonly CommandWarning[],
    scope: { accountId: string; area: AccountRefreshNotice['area'] } | { provider?: AccountProvider }
  ) => {
    const accountId = 'accountId' in scope ? scope.accountId : undefined
    const area = 'area' in scope ? scope.area : 'quota'
    const { byAccount, general } = splitRefreshWarnings(warnings, accountId)
    const accounts = getData()?.accounts ?? []
    setRefreshNotices((previous) => {
      if (previous.size === 0 && byAccount.size === 0) return previous
      const next = new Map(previous)
      for (const [id, { account, notice }] of previous) {
        const inScope = 'accountId' in scope
          ? id === scope.accountId
          : scope.provider === undefined || (account.provider ?? 'codex') === scope.provider
        if (inScope && notice.area === area) next.delete(id)
      }
      for (const account of accounts) {
        const warning = byAccount.get(account.id)
        if (warning) next.set(account.id, { account, notice: { ...warning, area } })
      }
      return next
    })
    return general
  }, [getData])

  const getRefreshNotice = useCallback((account: Account): AccountRefreshNotice | undefined => {
    const entry = refreshNotices.get(account.id)
    return entry?.account === account ? entry.notice : undefined
  }, [refreshNotices])

  const removeAccount = useCallback(async (accountId: string) => {
    const provider = getAccountProvider(accountId)
    const key: BusyKey = `delete:${accountId}`
    beginOp(key)
    try {
      clearError(provider)
      const next = await api.removeAccount(accountId)
      setData(next)
      return true
    } catch (err) {
      setProviderError(provider, describeIpcError(err))
      return false
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
      if (isGemini) window.dispatchEvent(new Event('antigravity-surfaces-changed'))
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
          setData((latest) => applyServerAccountOrder(latest, next))
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

  const refreshAccountData = useCallback(async (
    accountId: string,
    key: BusyKey,
    area: AccountRefreshNotice['area'],
    request: (id: string) => Promise<AccountRefreshResult>
  ) => {
    const provider = getAccountProvider(accountId)
    const scope = { accountId, area: provider === 'gemini' ? 'quota' as const : area }
    beginOp(key)
    try {
      const response = await request(accountId)
      setData((latest) => mergeAccountUpdates(latest, [response.account]))
      applyRefreshNotices(response.warnings, scope)
    } catch (err) {
      applyRefreshNotices([{
        code: 'refresh_failed', domain: 'account', message: describeIpcError(err), accountId, retryable: true
      }], scope)
    } finally {
      endOp(key)
    }
  }, [applyRefreshNotices, beginOp, endOp, getAccountProvider, setData])

  const detectSubscription = useCallback((accountId: string) => (
    refreshAccountData(accountId, `subscription-detect:${accountId}`, 'subscription', api.refreshAccountSubscription)
  ), [refreshAccountData])

  const refreshAccount = useCallback((accountId: string) => (
    refreshAccountData(accountId, `quota:${accountId}`, 'quota', api.refreshAccountQuota)
  ), [refreshAccountData])

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
    const targets: AccountProvider[] = provider ? [provider] : ['codex', 'gemini']
    if (targets.some((target) => refreshAllInFlightRef.current.has(target))) return
    targets.forEach((target) => refreshAllInFlightRef.current.add(target))
    setRefreshingProviders(new Set(refreshAllInFlightRef.current))
    const currentProv: AccountProvider = provider ?? 'codex'
    try {
      setProviderError(currentProv, null)
      const response = await api.refreshAllQuotas(provider)
      setData(response.state)
      const general = applyRefreshNotices(response.warnings, { provider })
      if (general.length > 0) setProviderError(currentProv, general.join(' · '))
    } catch (err) {
      setProviderError(currentProv, describeIpcError(err))
    } finally {
      targets.forEach((target) => refreshAllInFlightRef.current.delete(target))
      setRefreshingProviders(new Set(refreshAllInFlightRef.current))
    }
  }, [applyRefreshNotices, setData, setProviderError])

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

  const importCodex = useCallback(async () => {
    const key: BusyKey = 'import:codex'
    beginOp(key)
    try {
      setProviderError('codex', null)
      const response = await api.importCodexAccount()
      setData(response.state)
      reportWarnings(response.warnings, 'codex')
    } catch (err) {
      setProviderError('codex', describeIpcError(err))
    } finally {
      endOp(key)
    }
  }, [beginOp, endOp, reportWarnings, setData, setProviderError])

  return {
    busyKeys,
    refreshingProviders,
    errors,
    error: errors.codex ?? errors.gemini,
    getError,
    getRefreshNotice,
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
    importAntigravity,
    importCodex
  }
}
