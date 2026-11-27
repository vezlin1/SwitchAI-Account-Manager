import { Suspense, lazy, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { api } from '../../api'
import { AccountTagsDialog, type TagEditorTarget } from './AccountTags'
import type { Account, AccountProvider, AppData, AutoRefreshStatus, Recommendation } from '../../types'
import { useSharedTicker } from '../../hooks/useSharedTicker'
import { useAccountsActions } from '../../hooks/useAccountsActions'
import type { AppDataUpdater } from '../../hooks/useAppData'
import { useOAuthLogin } from '../../hooks/useOAuthLogin'
import { ErrorBanner } from '../common'
import { AccountsTable } from './AccountsTable'
import { AccountsToolbar } from './AccountsToolbar'
import { GeminiSwitchTargetsBar } from './GeminiSwitchTargetsBar'
import { BestAccountBar } from './BestAccountBar'
import {
  reorderFilteredAccounts,
  filterAccountsBySubscription,
  subscriptionFiltersForAccounts,
  type SubscriptionFilterId
} from '../../utils'
import { usePrivacy } from '../../context/usePrivacy'

const ConfirmDialog = lazy(() =>
  import('../common/ConfirmDialog').then((m) => ({ default: m.ConfirmDialog }))
)
const AccountDetails = lazy(() =>
  import('./AccountDetails').then((m) => ({ default: m.AccountDetails }))
)

type AccountsTabProps = {
  data: AppData
  setData: (next: AppDataUpdater) => AppData | null
  getData: () => AppData | null
  saveAppSettings: (settings: AppData['appSettings']) => Promise<AppData | null>
  reload: () => Promise<void>
  reloadAccountOrder: (shouldApply: () => boolean) => Promise<void>
  autoRefreshStatus: AutoRefreshStatus | null
  autoRefreshError?: string | null
  onClearAutoRefreshError?: () => void
  activeProvider?: AccountProvider
  settingsOpen: boolean
}

export function AccountsTab({
  data,
  setData,
  getData,
  saveAppSettings,
  reload,
  reloadAccountOrder,
  autoRefreshStatus,
  autoRefreshError,
  onClearAutoRefreshError,
  activeProvider = 'codex',
  settingsOpen
}: AccountsTabProps) {
  const [pendingDelete, setPendingDelete] = useState<Account | null>(null)
  const [pendingSwitch, setPendingSwitch] = useState<Account | null>(null)
  const [detailsAccountId, setDetailsAccountId] = useState<string | null>(null)
  const [tagEditor, setTagEditor] = useState<TagEditorTarget | null>(null)
  const editAccountTags = useCallback((account: Account) => setTagEditor({ account }), [])
  const [subscriptionFilters, setSubscriptionFilters] = useState<Record<AccountProvider, SubscriptionFilterId>>({
    codex: 'all',
    gemini: 'all'
  })
  const [searchTerms, setSearchTerms] = useState<Record<AccountProvider, string>>({
    codex: '',
    gemini: ''
  })
  const detailsReturnFocusRef = useRef<HTMLElement | null>(null)
  const reorderVersionRef = useRef(0)
  const pendingSwitchResolveRef = useRef<((confirmed: boolean) => void) | null>(null)
  const actions = useAccountsActions({
    setData,
    getData,
    persistAppSettings: saveAppSettings,
    confirmSwitch: async (account) => {
      setPendingSwitch(account)
      return new Promise<boolean>((resolve) => {
        pendingSwitchResolveRef.current = resolve
      })
    }
  })
  const clearAccountError = actions.clearError
  const accountBusyKeys = useMemo(() => {
    const keys = new Set(actions.busyKeys)
    for (const id of autoRefreshStatus?.refreshingAccountIds ?? []) keys.add(`account:${id}:quota`)
    return keys
  }, [actions.busyKeys, autoRefreshStatus?.refreshingAccountIds])
  const oauth = useOAuthLogin({ onCompleted: reload })
  const { privacyMode, maskEmail } = usePrivacy()
  const currentProvider = activeProvider ?? 'codex'
  const searchTerm = searchTerms[currentProvider] ?? ''
  const setSearchTerm = (val: string) => setSearchTerms((prev) => ({ ...prev, [currentProvider]: val }))
  const subscriptionFilter = subscriptionFilters[currentProvider] ?? 'all'
  const setSubscriptionFilter = (filter: SubscriptionFilterId) => {
    setDetailsAccountId(null)
    setSubscriptionFilters((prev) => ({ ...prev, [currentProvider]: filter }))
  }

  const accounts = useMemo(
    () => data.accounts.filter((account) => (account.provider ?? 'codex') === currentProvider),
    [data.accounts, currentProvider]
  )
  const activeAccountId = currentProvider === 'gemini'
    ? (data.activeGeminiAccountId ?? null)
    : data.activeAccountId
  const hiddenAccountIds = data.appSettings.hiddenAccountIds
  const tags = useMemo(() => (data.appSettings.accountTags ?? []).filter(t => t.provider === currentProvider),
    [data.appSettings.accountTags, currentProvider])
  const filters = useMemo(
    () => subscriptionFiltersForAccounts(accounts, hiddenAccountIds, tags),
    [accounts, hiddenAccountIds, tags]
  )
  const effectiveSubscriptionFilter = filters.some((filter) => filter.id === subscriptionFilter)
    ? subscriptionFilter
    : 'all'
  const openGroup = !settingsOpen && (effectiveSubscriptionFilter === 'hidden' || effectiveSubscriptionFilter.startsWith('tag:'))
    ? effectiveSubscriptionFilter : null
  const tabSync = useRef<Promise<void>>(Promise.resolve())
  useLayoutEffect(() => {
    const publish = (tab: string | null) => {
      tabSync.current = tabSync.current.catch(() => undefined).then(() => api.setOpenAccountTab(currentProvider, tab))
      void tabSync.current.catch(() => undefined)
    }
    const sync = () => publish(document.visibilityState === 'visible' ? openGroup : null)
    sync()
    document.addEventListener('visibilitychange', sync)
    return () => {
      document.removeEventListener('visibilitychange', sync)
      publish(null)
    }
  }, [currentProvider, openGroup])
  const filteredAccounts = useMemo(
    () => filterAccountsBySubscription(
      accounts,
      effectiveSubscriptionFilter,
      hiddenAccountIds, tags
    ),
    [accounts, effectiveSubscriptionFilter, hiddenAccountIds, tags]
  )

  const searchedAccounts = useMemo(() => {
    if (!searchTerm.trim()) return filteredAccounts
    const q = searchTerm.toLowerCase().trim()
    return filteredAccounts.filter((account) => {
      const email = (account.email ?? '').toLowerCase()
      const plan = (account.subscriptionPlan ?? '').toLowerCase()
      const accountId = (account.accountId ?? '').toLowerCase()
      return email.includes(q) || plan.includes(q) || accountId.includes(q)
    })
  }, [filteredAccounts, searchTerm])
  const detailsAccount = detailsAccountId
    ? filteredAccounts.find((account) => account.id === detailsAccountId) ?? null
    : null
  // The backend owns the recommendation so the window and the tray never
  // disagree. Re-ask when the data changes and once a minute, because quota
  // freshness and passed resets change with time alone.
  const [recommendation, setRecommendation] = useState<Recommendation | null>(null)
  const recommendationMinute = useSharedTicker()
  const geminiQuotaFocus = data.appSettings.geminiQuotaFocus
  useEffect(() => {
    let cancelled = false
    api.getRecommendedAccount(currentProvider)
      .then((next) => { if (!cancelled) setRecommendation(next) })
      .catch(() => { if (!cancelled) setRecommendation(null) })
    return () => { cancelled = true }
  }, [accounts, currentProvider, geminiQuotaFocus, hiddenAccountIds, recommendationMinute])
  const bestBarAccounts = useMemo(
    () => accounts.filter((account) => !hiddenAccountIds.includes(account.id)),
    [accounts, hiddenAccountIds]
  )
  const autoRefreshErrorForProvider = autoRefreshError
    ? ((autoRefreshError.toLowerCase().includes('gemini') && currentProvider === 'gemini') ||
       (autoRefreshError.toLowerCase().includes('chatgpt') && currentProvider === 'codex') ||
       (!autoRefreshError.toLowerCase().includes('gemini') && !autoRefreshError.toLowerCase().includes('chatgpt')))
      ? autoRefreshError
      : null
    : null

  const visibleError =
    actions.getError(currentProvider) ??
    oauth.getError(currentProvider) ??
    autoRefreshErrorForProvider

  const handleDismissError = () => {
    actions.clearError(currentProvider)
    oauth.clearError(currentProvider)
    onClearAutoRefreshError?.()
  }
  const openDetails = useCallback((account: Account, opener: HTMLElement) => {
    detailsReturnFocusRef.current = opener
    setDetailsAccountId(account.id)
  }, [])

  const toggleAccountInAll = (accountId: string) => {
    const next = setData((latest) => {
      const hidden = new Set(latest.appSettings.hiddenAccountIds)
      if (hidden.has(accountId)) hidden.delete(accountId)
      else hidden.add(accountId)

      return {
        ...latest,
        appSettings: {
          ...latest.appSettings,
          hiddenAccountIds: [...hidden]
        }
      }
    })
    if (next) void actions.saveAppSettings(next.appSettings)
  }

  const handleSwitch = useCallback(
    (account: Account) => {
      void actions.switchAccount(account)
    },
    [actions]
  )

  const handleRelogin = useCallback(
    (account: Account) => {
      void oauth.startLogin(account, account.provider ?? currentProvider)
    },
    [oauth, currentProvider]
  )

  const handleRemove = useCallback(
    (account: Account) => {
      clearAccountError(account.provider ?? currentProvider)
      setPendingDelete(account)
    },
    [clearAccountError, currentProvider]
  )

  const closeDetails = () => {
    setDetailsAccountId(null)
    window.requestAnimationFrame(() => detailsReturnFocusRef.current?.focus())
  }

  const reorderAccounts = async (activeId: string, overId: string) => {
    if (searchTerm.trim()) return
    const current = getData()
    if (!current) return
    const nextAccounts = reorderFilteredAccounts(current.accounts, filteredAccounts, activeId, overId)
    if (nextAccounts === current.accounts) {
      return
    }

    const reorderVersion = reorderVersionRef.current + 1
    reorderVersionRef.current = reorderVersion
    setData((latest) => ({
      ...latest,
      accounts: reorderFilteredAccounts(latest.accounts, filteredAccounts, activeId, overId)
    }))

    try {
      await actions.saveOrder(nextAccounts, currentProvider)
    } catch {
      await reloadAccountOrder(() => reorderVersionRef.current === reorderVersion)
    }
  }

  const specificCategoryCount = filters.filter((filter) => filter.id !== 'all' && filter.id !== 'hidden').length
  const totalHiddenCount = filters.find((f) => f.id === 'hidden')?.count ?? 0
  const showSubscriptionFilters = specificCategoryCount > 1 || totalHiddenCount > 0 || tags.length > 0

  return (
    <div className="page-fade h-full min-w-0 flex flex-col gap-4">
      {tagEditor && <AccountTagsDialog target={tagEditor} tags={data.appSettings.accountTags ?? []}
        onClose={() => setTagEditor(null)}
        onSave={accountTags => saveAppSettings({ ...(getData()?.appSettings ?? data.appSettings), accountTags })} />}
      {pendingDelete && (
        <Suspense fallback={null}>
          <ConfirmDialog
            title="Delete account?"
            message={`Delete ${
              privacyMode && pendingDelete.email
                ? maskEmail(pendingDelete.email)
                : (pendingDelete.email ?? 'this account')
            } from SwitchAI? This permanently deletes stored tokens and settings for this account.`}
            confirmLabel="Delete"
            variant="danger"
            busy={actions.busyKeys.has(`delete:${pendingDelete.id}`)}
            error={actions.getError(pendingDelete.provider ?? 'codex')}
            onCancel={() => setPendingDelete(null)}
            onConfirm={async () => {
              if (await actions.removeAccount(pendingDelete.id)) setPendingDelete(null)
            }}
          />
        </Suspense>
      )}

      {pendingSwitch && (
        <Suspense fallback={null}>
          <ConfirmDialog
            title={`Switch to ${
              privacyMode && pendingSwitch.email
                ? maskEmail(pendingSwitch.email)
                : (pendingSwitch.email ?? 'this account')
            }?`}
            message={pendingSwitch.provider === 'gemini'
              ? 'The Antigravity apps you picked close and reopen with this account.'
              : 'ChatGPT closes and reopens with this account if it is open.'}
            confirmLabel="Switch"
            busy={actions.busyKeys.has(`switch:${pendingSwitch.id}`)}
            onCancel={() => {
              pendingSwitchResolveRef.current?.(false)
              pendingSwitchResolveRef.current = null
              setPendingSwitch(null)
            }}
            onConfirm={() => {
              pendingSwitchResolveRef.current?.(true)
              pendingSwitchResolveRef.current = null
              setPendingSwitch(null)
            }}
          />
        </Suspense>
      )}

      {detailsAccount ? (
        <div className="flex flex-col gap-3">
          {visibleError && (
            <ErrorBanner
              message={visibleError}
              onDismiss={handleDismissError}
            />
          )}
          <Suspense fallback={null}>
            <AccountDetails
              account={detailsAccount}
              refreshNotice={actions.getRefreshNotice(detailsAccount)}
              isActive={activeAccountId === detailsAccount.id}
              recommendation={recommendation?.accountId === detailsAccount.id ? recommendation : null}
              busyKeys={accountBusyKeys}
              onBack={closeDetails}
              onEditTags={editAccountTags}
              onSwitch={handleSwitch}
              onRelogin={handleRelogin}
              onRefreshQuota={actions.refreshAccount}
              onDetectSubscription={actions.detectSubscription}
            />
          </Suspense>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <AccountsToolbar
            accountCount={accounts.length}
            provider={currentProvider}
            refreshingAll={actions.refreshingProviders.has(currentProvider)}
            addingAccount={oauth.isProviderBusy(currentProvider)}
            importingSession={
              currentProvider === 'gemini'
                ? actions.busyKeys.has('import:antigravity')
                : actions.busyKeys.has('import:codex')
            }
            appSettings={data.appSettings}
            autoRefreshStatus={autoRefreshStatus}
            searchTerm={searchTerm}
            onSearchChange={setSearchTerm}
            onAddAccount={() => void oauth.startLogin(undefined, currentProvider)}
            onCancelAddAccount={() => oauth.cancelLogin(currentProvider)}
            onImportSession={
              currentProvider === 'gemini'
                ? () => void actions.importAntigravity()
                : () => void actions.importCodex()
            }
            onRefreshAll={() => void actions.refreshAll(currentProvider, openGroup, tabSync.current)}
          />

          {currentProvider === 'gemini' && (
            <GeminiSwitchTargetsBar
              appSettings={data.appSettings}
              onSaveAppSettings={(settings) => actions.saveAppSettings(settings, 'gemini')}
            />
          )}

          {visibleError && (
            <ErrorBanner
              message={visibleError}
              onDismiss={handleDismissError}
            />
          )}

          <BestAccountBar
            provider={currentProvider}
            accounts={bestBarAccounts}
            activeAccountId={activeAccountId}
            recommendation={recommendation}
            switchingAccountId={recommendation && actions.busyKeys.has(`switch:${recommendation.accountId}`) ? recommendation.accountId : null}
            onSwitch={handleSwitch}
          />

          {showSubscriptionFilters && (
            <nav className="account-filter-bar" aria-label="Filter accounts">
              <div className="account-filter-summary">
                <span>Accounts</span>
                <span>{filteredAccounts.length} of {accounts.length}</span>
              </div>
              <div className="account-filter-options">
                {filters.map((filter) => {
                  const tag = tags.find(t => t.id === filter.id)
                  return (
                    <button
                      key={filter.id}
                      type="button"
                      className="account-filter-button"
                      aria-pressed={effectiveSubscriptionFilter === filter.id}
                      onClick={() => setSubscriptionFilter(filter.id)}
                      title={tag ? `${tag.name} · Right-click for tag settings` : undefined}
                      onContextMenu={tag ? event => { event.preventDefault(); setTagEditor({ tag }) } : undefined}
                      onKeyDown={tag ? event => {
                        if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
                          event.preventDefault()
                          setTagEditor({ tag })
                        }
                      } : undefined}
                    >
                      <span>{filter.label}</span>
                      <span className="account-filter-count">{filter.count}</span>
                    </button>
                  )
                })}
              </div>
            </nav>
          )}

          <AccountsTable
            accounts={searchedAccounts}
            tags={tags}
            provider={currentProvider}
            totalAccountCount={accounts.length}
            activeFilter={effectiveSubscriptionFilter}
            hiddenAccountCount={totalHiddenCount}
            addingAccount={oauth.isProviderBusy(currentProvider)}
            isSearching={Boolean(searchTerm.trim())}
            importingSession={
              currentProvider === 'gemini'
                ? actions.busyKeys.has('import:antigravity')
                : actions.busyKeys.has('import:codex')
            }
            activeAccountId={activeAccountId}
            busyKeys={accountBusyKeys}
            refreshingAll={actions.refreshingProviders.has(currentProvider)}
            autoRefreshing={Boolean(autoRefreshStatus?.progress?.some((run) => run.provider === currentProvider))}
            hiddenAccountIds={hiddenAccountIds}
            onReorder={(activeId, overId) => void reorderAccounts(activeId, overId)}
            onOpenDetails={openDetails}
            onEditTags={editAccountTags}
            onSwitch={handleSwitch}
            onRelogin={handleRelogin}
            getRefreshNotice={actions.getRefreshNotice}
            onRefreshQuota={actions.refreshAccount}
            onDetectSubscription={actions.detectSubscription}
            onRemove={handleRemove}
            onToggleAccountInAll={toggleAccountInAll}
            onAddAccount={() => void oauth.startLogin(undefined, currentProvider)}
            onImportSession={
              currentProvider === 'gemini'
                ? () => void actions.importAntigravity()
                : () => void actions.importCodex()
            }
            onClearFilter={() => {
              setSubscriptionFilter('all')
              setSearchTerm('')
            }}
            onShowHiddenAccounts={() => {
              const currentProviderAccountIds = new Set(accounts.map((a) => a.id))
              const next = setData((latest) => ({
                ...latest,
                appSettings: {
                  ...latest.appSettings,
                  hiddenAccountIds: latest.appSettings.hiddenAccountIds.filter(
                    (id) => !currentProviderAccountIds.has(id)
                  )
                }
              }))
              if (next) void actions.saveAppSettings(next.appSettings)
            }}
          />
        </div>
      )}
    </div>
  )
}
