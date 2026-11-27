import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type MouseEvent as ReactMouseEvent } from 'react'
import { computeAccountStatusFlags } from './useAccountRowState'
import {
  closestCenter,
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent
} from '@dnd-kit/core'
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { ArrowDownToLine, Loader2, LogIn, Sparkles, UserPlus, Users } from 'lucide-react'
import type { Account, AccountProvider, AccountTag } from '../../types'
import type { AccountRefreshNotice } from '../../utils/accountFeedback'
import { AccountCard } from './AccountCard'
import { AccountContextMenu } from './AccountContextMenu'
import { AccountRow } from './AccountRow'
import { formatSubscriptionPlan } from '../../utils/dateUtils'
import { quotaColumnsForAccounts, type QuotaColumn } from '../../utils/quotaWindows'
import type { SubscriptionFilterId } from '../../utils/subscriptionFilters'
import { usePrivacy } from '../../context/usePrivacy'

type AccountsTableProps = {
  accounts: Account[]
  tags: AccountTag[]
  provider?: AccountProvider
  activeAccountId: string | null
  totalAccountCount: number
  activeFilter: SubscriptionFilterId
  hiddenAccountCount: number
  addingAccount: boolean
  isSearching?: boolean
  importingSession?: boolean
  busyKeys: ReadonlySet<string>
  refreshingAll: boolean
  autoRefreshing: boolean
  hiddenAccountIds: string[]
  onReorder: (activeId: string, overId: string) => void
  onOpenDetails: (account: Account, opener: HTMLElement) => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  getRefreshNotice: (account: Account) => AccountRefreshNotice | undefined
  onRefreshQuota: (accountId: string) => Promise<void>
  onDetectSubscription: (accountId: string) => Promise<void>
  onRemove: (account: Account) => void
  onToggleAccountInAll: (accountId: string) => void
  onEditTags: (account: Account) => void
  onAddAccount: () => void
  onImportSession?: () => void
  onClearFilter: () => void
  onShowHiddenAccounts: () => void
}

function AccountsEmptyState({
  accountCount,
  provider = 'codex',
  activeFilter,
  hiddenAccountCount,
  addingAccount,
  isSearching = false,
  importingSession = false,
  onAddAccount,
  onImportSession,
  onClearFilter,
  onShowHiddenAccounts
}: {
  accountCount: number
  provider?: AccountProvider
  activeFilter: SubscriptionFilterId
  hiddenAccountCount: number
  addingAccount: boolean
  isSearching?: boolean
  importingSession?: boolean
  onAddAccount: () => void
  onImportSession?: () => void
  onClearFilter: () => void
  onShowHiddenAccounts: () => void
}) {
  const isGoogle = provider === 'gemini'

  if (accountCount === 0) {
    return (
      <div className="accounts-empty py-16 px-6 text-center flex flex-col items-center justify-center max-w-md mx-auto" role="status">
        <div className="w-12 h-12 rounded-2xl bg-ag-surface border border-ag-border flex items-center justify-center text-ag-primary mb-4 shadow-sm">
          {isGoogle ? <Sparkles size={22} /> : <Users size={22} />}
        </div>
        <h3 className="text-base font-semibold text-ag-text mb-1.5">
          {isGoogle ? 'No Gemini accounts added yet' : 'No ChatGPT accounts added yet'}
        </h3>
        <p className="text-xs text-ag-muted leading-relaxed mb-5">
          {isGoogle
            ? 'Sign in with your Google account or import your Antigravity session to start managing quotas.'
            : 'Add your first ChatGPT account to start tracking weekly limits, switching sessions, and managing subscriptions.'}
        </p>
        <div className="flex items-center gap-2.5 flex-wrap justify-center">
          <button
            type="button"
            className="h-9 px-4 rounded-lg bg-ag-primary text-white text-xs font-semibold hover:bg-blue-600 inline-flex items-center gap-2 disabled:opacity-60 transition-all shadow-sm cursor-pointer"
            onClick={onAddAccount}
            disabled={addingAccount || importingSession}
          >
            {addingAccount
              ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
              : isGoogle
                ? <LogIn size={14} aria-hidden="true" />
                : <UserPlus size={14} aria-hidden="true" />}
            {isGoogle ? 'Sign in with Google' : 'Add account'}
          </button>
          {onImportSession && (
            <button
              type="button"
              className="h-9 px-4 rounded-lg bg-ag-surface border border-ag-border text-ag-text text-xs font-semibold hover:bg-white/[0.06] inline-flex items-center gap-2 disabled:opacity-60 transition-all shadow-sm cursor-pointer"
              onClick={onImportSession}
              disabled={addingAccount || importingSession}
            >
              {importingSession ? (
                <Loader2 size={14} className="animate-spin" aria-hidden="true" />
              ) : (
                <ArrowDownToLine size={14} aria-hidden="true" />
              )}
              Import active session
            </button>
          )}
        </div>
      </div>
    )
  }

  if (isSearching) {
    return (
      <div className="accounts-empty py-14 px-6 text-center flex flex-col items-center justify-center max-w-md mx-auto" role="status">
        <h3 className="text-sm font-semibold text-ag-text mb-1">No accounts match your search</h3>
        <p className="text-xs text-ag-muted mb-4">Check the spelling or clear the search query to view all accounts.</p>
        <button type="button" className="h-8 px-3.5 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface transition-all cursor-pointer" onClick={onClearFilter}>
          Clear search and filters
        </button>
      </div>
    )
  }

  if (activeFilter === 'all' && hiddenAccountCount < accountCount) {
    return (
      <div className="accounts-empty py-14 px-6 text-center" role="status">
        <h3 className="text-sm font-semibold text-ag-text mb-1">No accounts shown in All</h3>
        <p className="text-xs text-ag-muted">Open a tag to see its accounts or change its visibility in tag settings.</p>
      </div>
    )
  }

  if (activeFilter === 'all' && hiddenAccountCount > 0 && hiddenAccountCount >= accountCount) {
    return (
      <div className="accounts-empty py-14 px-6 text-center flex flex-col items-center justify-center max-w-md mx-auto" role="status">
        <h3 className="text-sm font-semibold text-ag-text mb-1">All accounts are hidden</h3>
        <p className="text-xs text-ag-muted mb-4">Every account is hidden from the All filter.</p>
        <button type="button" className="h-8 px-3.5 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface transition-all cursor-pointer" onClick={onShowHiddenAccounts}>
          Show all hidden accounts
        </button>
      </div>
    )
  }

  return (
    <div className="accounts-empty py-14 px-6 text-center flex flex-col items-center justify-center max-w-md mx-auto" role="status">
      <h3 className="text-sm font-semibold text-ag-text mb-1">No accounts match this filter</h3>
      <p className="text-xs text-ag-muted mb-4">{activeFilter.startsWith('tag:')
        ? 'Assign this tag from the account’s Edit tags menu.'
        : 'Choose a different subscription filter or show all accounts.'}</p>
      <button type="button" className="h-8 px-3.5 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface transition-all cursor-pointer" onClick={onClearFilter}>
        Show all accounts
      </button>
    </div>
  )
}

const ACCOUNT_PAGE_SIZE = 40

/**
 * Rows are dragged by their whole surface. A drag never starts from a field the
 * user types or picks in, so editing a subscription date stays editing.
 */
class RowPointerSensor extends PointerSensor {
  static activators = [{
    eventName: 'onPointerDown' as const,
    handler: ({ nativeEvent }: { nativeEvent: PointerEvent }) => {
      if (!nativeEvent.isPrimary || nativeEvent.button !== 0) return false
      const target = nativeEvent.target
      return !(target instanceof Element && target.closest('input, textarea, select, [contenteditable="true"], [data-no-drag]'))
    }
  }]
}

export const AccountsTable = memo(function AccountsTable({
  accounts,
  tags,
  provider = 'codex',
  activeAccountId,
  totalAccountCount,
  activeFilter,
  hiddenAccountCount,
  addingAccount,
  isSearching = false,
  importingSession = false,
  busyKeys,
  refreshingAll,
  autoRefreshing,
  hiddenAccountIds,
  onReorder,
  onOpenDetails,
  onSwitch,
  onRelogin,
  getRefreshNotice,
  onRefreshQuota,
  onDetectSubscription,
  onRemove,
  onToggleAccountInAll,
  onEditTags,
  onAddAccount,
  onImportSession,
  onClearFilter,
  onShowHiddenAccounts
}: AccountsTableProps) {
  const [activeDragId, setActiveDragId] = useState<string | null>(null)
  const [visibleCount, setVisibleCount] = useState(ACCOUNT_PAGE_SIZE)
  const loadMoreRef = useRef<HTMLButtonElement>(null)
  const visibleAccounts = useMemo(() => accounts.slice(0, visibleCount), [accounts, visibleCount])
  const canLoadMore = visibleCount < accounts.length
  const hiddenIds = useMemo(() => new Set(hiddenAccountIds), [hiddenAccountIds])
  const tagNamesByAccount = useMemo(() => {
    const labels = new Map<string, string[]>()
    for (const tag of tags) {
      for (const id of tag.accountIds) {
        const names = labels.get(id)
        if (names) names.push(tag.name)
        else labels.set(id, [tag.name])
      }
    }
    return labels
  }, [tags])
  const loadMore = useCallback(() => setVisibleCount((count) => count + ACCOUNT_PAGE_SIZE), [])
  const [contextMenu, setContextMenu] = useState<{
    accountId: string
    x: number
    y: number
    opener: HTMLElement
    keyboardTriggered: boolean
  } | null>(null)
  // These small schemas depend on ids/window kinds, not the changing quota
  // values. Scalar dependencies keep SortableContext and unchanged rows stable.
  const accountIdsJson = useMemo(() => JSON.stringify(visibleAccounts.map((account) => account.id)), [visibleAccounts])
  const accountIds = useMemo(() => JSON.parse(accountIdsJson) as string[], [accountIdsJson])
  const quotaColumnsJson = useMemo(() => JSON.stringify(quotaColumnsForAccounts(accounts)), [accounts])
  const quotaColumns = useMemo(() => JSON.parse(quotaColumnsJson) as QuotaColumn[], [quotaColumnsJson])
  const activeDragAccount = activeDragId
    ? accounts.find((account) => account.id === activeDragId) ?? null
    : null
  const contextAccount = contextMenu
    ? accounts.find((account) => account.id === contextMenu.accountId) ?? null
    : null
  const { privacyMode, maskEmail } = usePrivacy()
  const orderBusy = busyKeys.has('order')
  const sensors = useSensors(
    useSensor(RowPointerSensor, {
      activationConstraint: { distance: 6 }
    })
  )
  const [isMobileView, setIsMobileView] = useState(() =>
    typeof window !== 'undefined' ? window.matchMedia('(max-width: 860px)').matches : false
  )

  useEffect(() => {
    const button = loadMoreRef.current
    if (!button || typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting && document.activeElement !== button) {
        observer.disconnect()
        loadMore()
      }
    }, { root: button.closest('main'), rootMargin: '0px 0px 480px' })
    observer.observe(button)
    return () => observer.disconnect()
  }, [canLoadMore, isMobileView, loadMore, visibleCount])

  useEffect(() => {
    if (typeof window === 'undefined') return
    const media = window.matchMedia('(max-width: 860px)')
    const handler = (e: MediaQueryListEvent) => setIsMobileView(e.matches)
    media.addEventListener('change', handler)
    return () => media.removeEventListener('change', handler)
  }, [])

  const desktopDraggingEnabled = accounts.length > 1 && !orderBusy && !isSearching

  const handleDragStart = (event: DragStartEvent) => {
    setActiveDragId(String(event.active.id))
  }

  const handleDragEnd = (event: DragEndEvent) => {
    const activeId = String(event.active.id)
    const overId = event.over ? String(event.over.id) : null
    setActiveDragId(null)

    if (overId && activeId !== overId) {
      onReorder(activeId, overId)
    }
  }

  const openContextMenu = useCallback((event: ReactMouseEvent<HTMLTableRowElement>, account: Account) => {
    event.preventDefault()
    const keyboardTriggered = event.clientX === 0 && event.clientY === 0
    const bounds = event.currentTarget.getBoundingClientRect()
    const target = event.target instanceof HTMLElement ? event.target : event.currentTarget
    const opener = target.closest<HTMLElement>('button')
      ?? event.currentTarget.querySelector<HTMLElement>('.account-identity-button')
      ?? event.currentTarget

    setContextMenu({
      accountId: account.id,
      x: keyboardTriggered ? bounds.left + Math.min(bounds.width - 16, 280) : event.clientX,
      y: keyboardTriggered ? bounds.top + 40 : event.clientY,
      opener,
      keyboardTriggered
    })
  }, [])

  const loadMoreButton = canLoadMore && (
    <button
      ref={loadMoreRef}
      type="button"
      className="account-detail-action mx-auto my-2"
      onClick={() => {
        const lastAccount = loadMoreRef.current?.closest('[data-load-more]')?.previousElementSibling
        loadMore()
        requestAnimationFrame(() => lastAccount?.nextElementSibling?.querySelector('button')?.focus())
      }}
    >
      Show more accounts
    </button>
  )

  return (
    <div className="accounts-table-shell">
      {accounts.length === 0 ? (
        <div className="accounts-table-empty">
          <AccountsEmptyState
            accountCount={totalAccountCount}
            provider={provider}
            activeFilter={activeFilter}
            hiddenAccountCount={hiddenAccountCount}
            addingAccount={addingAccount}
            isSearching={isSearching}
            importingSession={importingSession}
            onAddAccount={onAddAccount}
            onImportSession={onImportSession}
            onClearFilter={onClearFilter}
            onShowHiddenAccounts={onShowHiddenAccounts}
          />
        </div>
      ) : !isMobileView ? (
        <div className="accounts-desktop-view">
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            autoScroll={desktopDraggingEnabled}
            onDragStart={handleDragStart}
            onDragCancel={() => setActiveDragId(null)}
            onDragEnd={handleDragEnd}
          >
            <table
              className="accounts-table"
              aria-busy={orderBusy || refreshingAll || autoRefreshing}
              aria-rowcount={canLoadMore ? accounts.length + 1 : undefined}
            >
              <caption className="visually-hidden">Accounts with quota, status, and subscription details</caption>
              <thead>
                <tr>
                  <th scope="col">Account</th>
                  {quotaColumns.map((column) => (
                    <th key={column.key} scope="col">{column.label}</th>
                  ))}
                  <th scope="col">Plan</th>
                  <th scope="col" className="accounts-th-actions"><span className="visually-hidden">Actions</span></th>
                </tr>
              </thead>
              <SortableContext items={accountIds} strategy={verticalListSortingStrategy}>
                <tbody>
                  {visibleAccounts.map((account) => {
                    const statusFlags = computeAccountStatusFlags(
                      account.id,
                      busyKeys
                    )
                    return (
                      <SortableAccountRow
                        key={account.id}
                        account={account}
                        tagNames={tagNamesByAccount.get(account.id)?.join(' · ')}
                        refreshNotice={getRefreshNotice(account)}
                        isActive={activeAccountId === account.id}
                        isSwitching={statusFlags.isSwitching}
                        isRemoving={statusFlags.isRemoving}
                        isRelogining={statusFlags.isRelogining}
                        isRefreshing={statusFlags.isRefreshing}
                        hiddenFromAll={hiddenIds.has(account.id)}
                        quotaColumns={quotaColumns}
                        orderBusy={orderBusy || isSearching}
                        onOpenDetails={onOpenDetails}
                        onSwitch={onSwitch}
                        onRelogin={onRelogin}
                        onRefreshQuota={onRefreshQuota}
                        onDetectSubscription={onDetectSubscription}
                        onRemove={onRemove}
                        onOpenContextMenu={openContextMenu}
                      />
                    )
                  })}
                  {canLoadMore && (
                    <tr data-load-more>
                      <td colSpan={quotaColumns.length + 3}>{loadMoreButton}</td>
                    </tr>
                  )}
                </tbody>
              </SortableContext>
            </table>

            <DragOverlay dropAnimation={{ duration: 180, easing: 'cubic-bezier(0.2, 0, 0, 1)' }}>
              {activeDragAccount ? (
                <div className="account-row-drag-overlay">
                  <strong>
                    {(privacyMode && activeDragAccount.email
                      ? maskEmail(activeDragAccount.email)
                      : activeDragAccount.email) ?? 'Account'}
                  </strong>
                  <span>
                    {formatSubscriptionPlan(
                      activeDragAccount.subscriptionPlan ?? activeDragAccount.quota?.planType,
                      activeDragAccount.provider
                    ) ?? 'No plan reported'}
                  </span>
                </div>
              ) : null}
            </DragOverlay>
          </DndContext>
        </div>
      ) : (
        <div className="accounts-mobile-view">
          <ul className="account-card-list">
            {visibleAccounts.map((account) => {
              const statusFlags = computeAccountStatusFlags(
                account.id,
                busyKeys
              )
              return (
                <li key={account.id}>
                  <AccountCard
                    account={account}
                    tagNames={tagNamesByAccount.get(account.id)?.join(' · ')}
                    refreshNotice={getRefreshNotice(account)}
                    isActive={activeAccountId === account.id}
                    isSwitching={statusFlags.isSwitching}
                    isRemoving={statusFlags.isRemoving}
                    isRelogining={statusFlags.isRelogining}
                    isRefreshing={statusFlags.isRefreshing}
                    quotaColumns={quotaColumns}
                    hiddenFromAll={hiddenIds.has(account.id)}
                    onOpenDetails={onOpenDetails}
                    onSwitch={onSwitch}
                    onRelogin={onRelogin}
                    onRefreshQuota={onRefreshQuota}
                    onDetectSubscription={onDetectSubscription}
                    onRemove={onRemove}
                    onToggleInAll={onToggleAccountInAll}
                  />
                </li>
              )
            })}
            {canLoadMore && <li data-load-more>{loadMoreButton}</li>}
          </ul>
        </div>
      )}

      {contextMenu && contextAccount && (
        <AccountContextMenu
          onEditTags={onEditTags}
          account={contextAccount}
          hiddenFromAll={hiddenIds.has(contextAccount.id)}
          x={contextMenu.x}
          y={contextMenu.y}
          opener={contextMenu.opener}
          keyboardTriggered={contextMenu.keyboardTriggered}
          onToggleInAll={onToggleAccountInAll}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  )
})

type SortableAccountRowProps = {
  account: Account
  tagNames?: string
  refreshNotice?: AccountRefreshNotice
  isActive: boolean
  isSwitching: boolean
  isRemoving: boolean
  isRelogining: boolean
  isRefreshing: boolean
  hiddenFromAll: boolean
  quotaColumns: QuotaColumn[]
  orderBusy: boolean
  onOpenDetails: (account: Account, opener: HTMLElement) => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRefreshQuota: (accountId: string) => Promise<void>
  onDetectSubscription: (accountId: string) => Promise<void>
  onRemove: (account: Account) => void
  onOpenContextMenu: (event: ReactMouseEvent<HTMLTableRowElement>, account: Account) => void
}

const SortableAccountRow = memo(function SortableAccountRow({
  account,
  tagNames,
  refreshNotice,
  isActive,
  isSwitching,
  isRemoving,
  isRelogining,
  isRefreshing,
  hiddenFromAll,
  quotaColumns,
  orderBusy,
  onOpenDetails,
  onSwitch,
  onRelogin,
  onRefreshQuota,
  onDetectSubscription,
  onRemove,
  onOpenContextMenu
}: SortableAccountRowProps) {
  const {
    listeners,
    setActivatorNodeRef,
    setNodeRef,
    transform,
    transition,
    isDragging
  } = useSortable({
    id: account.id,
    disabled: orderBusy
  })

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    position: 'relative',
    zIndex: isDragging ? 2 : undefined,
    opacity: isDragging ? 0.45 : undefined
  }

  return (
    <AccountRow
      account={account}
      tagNames={tagNames}
      refreshNotice={refreshNotice}
      isActive={isActive}
      isSwitching={isSwitching}
      isRemoving={isRemoving}
      isRelogining={isRelogining}
      isRefreshing={isRefreshing}
      hiddenFromAll={hiddenFromAll}
      quotaColumns={quotaColumns}
      orderBusy={orderBusy}
      rowStyle={style}
      rowRef={setNodeRef}
      dragHandleListeners={listeners}
      setDragHandleRef={setActivatorNodeRef}
      onOpenDetails={onOpenDetails}
      onSwitch={onSwitch}
      onRelogin={onRelogin}
      onRefreshQuota={onRefreshQuota}
      onDetectSubscription={onDetectSubscription}
      onRemove={onRemove}
      onOpenContextMenu={onOpenContextMenu}
    />
  )
})
