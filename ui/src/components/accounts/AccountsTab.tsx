import { Suspense, lazy, useCallback, useMemo, useRef, useState } from 'react'
import type { Account, AccountProvider, AppData, AutoRefreshStatus } from '../../types'
import { useAccountsActions } from '../../hooks/useAccountsActions'
import type { AppDataUpdater } from '../../hooks/useAppData'
import { useOAuthLogin } from '../../hooks/useOAuthLogin'
import { ErrorBanner } from '../common'
import { AccountsTable } from './AccountsTable'
import { AccountsToolbar } from './AccountsToolbar'
import { GeminiSwitchTargetsBar } from './GeminiSwitchTargetsBar'
import {
  recommendedAccount,
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
  activeProvider = 'codex'
}: AccountsTabProps) {
  const [pendingDelete, setPendingDelete] = useState<Account | null>(null)
  const [pendingSwitch, setPendingSwitch] = useState<Account | null>(null)
  const [detailsAccountId, setDetailsAccountId] = useState<string | null>(null)
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
  const oauth = useOAuthLogin({ onCompleted: reload })
  const { privacyMode, maskEmail } = usePrivacy()
  const currentProvider = activeProvider ?? 'codex'
  const searchTerm = searchTerms[currentProvider] ?? ''
  const setSearchTerm = (val: string) => setSearchTerms((prev) => ({ ...prev, [currentProvider]: val }))
  const subscriptionFilter = subscriptionFilters[currentProvider] ?? 'all'
  const setSubscriptionFilter = (filter: SubscriptionFilterId) =>
    setSubscriptionFilters((prev) => ({ ...prev, [currentProvider]: filter }))

  const accounts = useMemo(
    () => data.accounts.filter((account) => (account.provider ?? 'codex') === currentProvider),
    [data.accounts, currentProvider]
  )
  const activeAccountId = currentProvider === 'gemini'
    ? (data.activeGeminiAccountId ?? null)
    : data.activeAccountId
  const hiddenAccountIds = data.appSettings.hiddenAccountIds
  const filters = useMemo(
    () => subscriptionFiltersForAccounts(accounts, hiddenAccountIds),
    [accounts, hiddenAccountIds]
  )
  const effectiveSubscriptionFilter = filters.some((filter) => filter.id === subscriptionFilter)
    ? subscriptionFilter
    : 'all'
  const filteredAccounts = useMemo(
    () => filterAccountsBySubscription(
      accounts,
      effectiveSubscriptionFilter,
      hiddenAccountIds
    ),
    [accounts, effectiveSubscriptionFilter, hiddenAccountIds]
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
    ? accounts.find((account) => account.id === detailsAccountId) ?? null
    : null
  const recommendation = useMemo(
    () => recommendedAccount(accounts, hiddenAccountIds),
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
      setPendingDelete(account)
    },
    []
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
  const showSubscriptionFilters = specificCategoryCount > 1 || totalHiddenCount > 0

  return (
    <div className="page-fade h-full min-w-0 flex flex-col gap-4">
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
            onCancel={() => setPendingDelete(null)}
            onConfirm={async () => {
              await actions.removeAccount(pendingDelete.id)
              setPendingDelete(null)
            }}
          />
        </Suspense>
      )}

      {pendingSwitch && (
        <Suspense fallback={null}>
          <ConfirmDialog
            title="Switch active account?"
            message={pendingSwitch.provider === 'gemini'
              ? `Switch active session to ${
                  privacyMode && pendingSwitch.email
                    ? maskEmail(pendingSwitch.email)
                    : (pendingSwitch.email ?? 'this account')
                }? Antigravity will restart with this account.`
              : `Switch active session to ${
                  privacyMode && pendingSwitch.email
                    ? maskEmail(pendingSwitch.email)
                    : (pendingSwitch.email ?? 'this account')
                }? Codex will restart with this account.`}
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
              isActive={activeAccountId === detailsAccount.id}
              isRecommended={recommendation?.id === detailsAccount.id}
              busyKeys={actions.busyKeys}
              refreshingAll={actions.refreshingAll}
              autoRefreshing={Boolean(autoRefreshStatus?.inFlight)}
              onBack={closeDetails}
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
            refreshingAll={actions.refreshingAll}
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
            onRefreshAll={() => void actions.refreshAll(currentProvider)}
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

          {accounts.length > 0 && showSubscriptionFilters && (
            <nav className="account-filter-bar" aria-label="Filter accounts by subscription">
              <div className="account-filter-summary">
                <span>Accounts</span>
                <span>{filteredAccounts.length} of {accounts.length}</span>
              </div>
              <div className="account-filter-options">
                {filters.map((filter) => (
                  <button
                    key={filter.id}
                    type="button"
                    className="account-filter-button"
                    aria-pressed={effectiveSubscriptionFilter === filter.id}
                    onClick={() => setSubscriptionFilter(filter.id)}
                  >
                    <span>{filter.label}</span>
                    <span className="account-filter-count">{filter.count}</span>
                  </button>
                ))}
              </div>
            </nav>
          )}

          <AccountsTable
            accounts={searchedAccounts}
            provider={currentProvider}
            totalAccountCount={accounts.length}
            activeFilter={effectiveSubscriptionFilter}
            hiddenAccountCount={hiddenAccountIds.length}
            addingAccount={oauth.isProviderBusy(currentProvider)}
            isSearching={Boolean(searchTerm.trim())}
            importingSession={
              currentProvider === 'gemini'
                ? actions.busyKeys.has('import:antigravity')
                : actions.busyKeys.has('import:codex')
            }
            activeAccountId={activeAccountId}
            busyKeys={actions.busyKeys}
            refreshingAll={actions.refreshingAll}
            autoRefreshing={Boolean(autoRefreshStatus?.inFlight)}
            hiddenAccountIds={hiddenAccountIds}
            onReorder={(activeId, overId) => void reorderAccounts(activeId, overId)}
            onOpenDetails={openDetails}
            onSwitch={handleSwitch}
            onRelogin={handleRelogin}
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
