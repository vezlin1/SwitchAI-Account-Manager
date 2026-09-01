import { memo, useEffect, useMemo, useState, type CSSProperties, type MouseEvent as ReactMouseEvent } from 'react'
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
import { Loader2, LogIn, Sparkles, UserPlus, Users } from 'lucide-react'
import type { Account, AccountProvider } from '../../types'
import { AccountCard } from './AccountCard'
import { AccountContextMenu } from './AccountContextMenu'
import { AccountRow } from './AccountRow'
import { formatSubscriptionPlan } from '../../utils/dateUtils'
import { quotaColumnsForAccounts, type QuotaColumn } from '../../utils/quotaWindows'
import type { SubscriptionFilterId } from '../../utils/subscriptionFilters'

type AccountsTableProps = {
  accounts: Account[]
  provider?: AccountProvider
  activeAccountId: string | null
  totalAccountCount: number
  activeFilter: SubscriptionFilterId
  hiddenAccountCount: number
  addingAccount: boolean
  busyKeys: ReadonlySet<string>
  refreshingAll: boolean
  autoRefreshing: boolean
  hiddenAccountIds: string[]
  onReorder: (activeId: string, overId: string) => void
  onOpenDetails: (account: Account, opener: HTMLElement) => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRemove: (account: Account) => void
  onToggleAccountInAll: (accountId: string) => void
  onAddAccount: () => void
  onClearFilter: () => void
  onShowHiddenAccounts: () => void
}

function AccountsEmptyState({
  accountCount,
  provider = 'codex',
  activeFilter,
  hiddenAccountCount,
  addingAccount,
  onAddAccount,
  onClearFilter,
  onShowHiddenAccounts
}: {
  accountCount: number
  provider?: AccountProvider
  activeFilter: SubscriptionFilterId
  hiddenAccountCount: number
  addingAccount: boolean
  onAddAccount: () => void
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
        <button
          type="button"
          className="h-9 px-4 rounded-lg bg-ag-primary text-white text-xs font-semibold hover:bg-blue-600 inline-flex items-center gap-2 disabled:opacity-60 transition-all shadow-sm cursor-pointer"
          onClick={onAddAccount}
          disabled={addingAccount}
        >
          {addingAccount
            ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
            : isGoogle
              ? <LogIn size={14} aria-hidden="true" />
              : <UserPlus size={14} aria-hidden="true" />}
          {isGoogle ? 'Sign in with Google' : 'Add account'}
        </button>
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
      <p className="text-xs text-ag-muted mb-4">Choose a different subscription filter or show all accounts.</p>
      <button type="button" className="h-8 px-3.5 rounded-lg border border-ag-border text-xs font-medium text-ag-text hover:bg-ag-surface transition-all cursor-pointer" onClick={onClearFilter}>
        Show all accounts
      </button>
    </div>
  )
}

export function AccountsTable({
  accounts,
  provider = 'codex',
  activeAccountId,
  totalAccountCount,
  activeFilter,
  hiddenAccountCount,
  addingAccount,
  busyKeys,
  refreshingAll,
  autoRefreshing,
  hiddenAccountIds,
  onReorder,
  onOpenDetails,
  onSwitch,
  onRelogin,
  onRemove,
  onToggleAccountInAll,
  onAddAccount,
  onClearFilter,
  onShowHiddenAccounts
}: AccountsTableProps) {
  const [activeDragId, setActiveDragId] = useState<string | null>(null)
  const [contextMenu, setContextMenu] = useState<{
    accountId: string
    x: number
    y: number
    opener: HTMLElement
    keyboardTriggered: boolean
  } | null>(null)
  const accountIds = useMemo(() => accounts.map((account) => account.id), [accounts])
  const quotaColumns = useMemo(() => quotaColumnsForAccounts(accounts), [accounts])
  const activeDragAccount = activeDragId
    ? accounts.find((account) => account.id === activeDragId) ?? null
    : null
  const contextAccount = contextMenu
    ? accounts.find((account) => account.id === contextMenu.accountId) ?? null
    : null
  const orderBusy = busyKeys.has('order')
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 6 }
    })
  )
  const [isMobileView, setIsMobileView] = useState(() =>
    typeof window !== 'undefined' ? window.matchMedia('(max-width: 860px)').matches : false
  )

  useEffect(() => {
    if (typeof window === 'undefined') return
    const media = window.matchMedia('(max-width: 860px)')
    const handler = (e: MediaQueryListEvent) => setIsMobileView(e.matches)
    media.addEventListener('change', handler)
    return () => media.removeEventListener('change', handler)
  }, [])

  const desktopDraggingEnabled = accounts.length > 1 && !orderBusy

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

  const openContextMenu = (event: ReactMouseEvent<HTMLTableRowElement>, account: Account) => {
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
  }

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
            onAddAccount={onAddAccount}
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
            <table className="accounts-table" aria-busy={orderBusy || refreshingAll || autoRefreshing}>
              <caption className="visually-hidden">Accounts with quota, status, and subscription details</caption>
              <thead>
                <tr>
                  <th scope="col" className="accounts-th-order">Order</th>
                  <th scope="col">Account</th>
                  {quotaColumns.map((column) => (
                    <th key={column.key} scope="col">{column.label}</th>
                  ))}
                  <th scope="col">Status</th>
                  <th scope="col">Subscription</th>
                  <th scope="col" className="accounts-th-actions">Actions</th>
                </tr>
              </thead>
              <SortableContext items={accountIds} strategy={verticalListSortingStrategy}>
                <tbody>
                  {accounts.map((account) => (
                    <SortableAccountRow
                      key={account.id}
                      account={account}
                      isActive={activeAccountId === account.id}
                      busyKeys={busyKeys}
                      refreshingAll={refreshingAll}
                      autoRefreshing={autoRefreshing}
                      hiddenFromAll={hiddenAccountIds.includes(account.id)}
                      quotaColumns={quotaColumns}
                      orderBusy={orderBusy}
                      onOpenDetails={onOpenDetails}
                      onSwitch={onSwitch}
                      onRelogin={onRelogin}
                      onRemove={onRemove}
                      onOpenContextMenu={openContextMenu}
                    />
                  ))}
                </tbody>
              </SortableContext>
            </table>

            <DragOverlay dropAnimation={{ duration: 180, easing: 'cubic-bezier(0.2, 0, 0, 1)' }}>
              {activeDragAccount ? (
                <div className="account-row-drag-overlay">
                  <strong>{activeDragAccount.email ?? 'Account'}</strong>
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
            {accounts.map((account) => (
              <li key={account.id}>
                <AccountCard
                  account={account}
                  isActive={activeAccountId === account.id}
                  busyKeys={busyKeys}
                  refreshingAll={refreshingAll}
                  autoRefreshing={autoRefreshing}
                  quotaColumns={quotaColumns}
                  hiddenFromAll={hiddenAccountIds.includes(account.id)}
                  onOpenDetails={onOpenDetails}
                  onSwitch={onSwitch}
                  onRelogin={onRelogin}
                  onRemove={onRemove}
                  onToggleInAll={onToggleAccountInAll}
                />
              </li>
            ))}
          </ul>
        </div>
      )}

      {contextMenu && contextAccount && (
        <AccountContextMenu
          account={contextAccount}
          hiddenFromAll={hiddenAccountIds.includes(contextAccount.id)}
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
}

type SortableAccountRowProps = {
  account: Account
  isActive: boolean
  busyKeys: ReadonlySet<string>
  refreshingAll: boolean
  autoRefreshing: boolean
  hiddenFromAll: boolean
  quotaColumns: QuotaColumn[]
  orderBusy: boolean
  onOpenDetails: (account: Account, opener: HTMLElement) => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRemove: (account: Account) => void
  onOpenContextMenu: (event: ReactMouseEvent<HTMLTableRowElement>, account: Account) => void
}

const SortableAccountRow = memo(function SortableAccountRow({
  account,
  isActive,
  busyKeys,
  refreshingAll,
  autoRefreshing,
  hiddenFromAll,
  quotaColumns,
  orderBusy,
  onOpenDetails,
  onSwitch,
  onRelogin,
  onRemove,
  onOpenContextMenu
}: SortableAccountRowProps) {
  const {
    attributes,
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
      isActive={isActive}
      busyKeys={busyKeys}
      refreshingAll={refreshingAll}
      autoRefreshing={autoRefreshing}
      hiddenFromAll={hiddenFromAll}
      quotaColumns={quotaColumns}
      orderBusy={orderBusy}
      rowStyle={style}
      rowRef={setNodeRef}
      dragHandleAttributes={attributes}
      dragHandleListeners={listeners}
      setDragHandleRef={setActivatorNodeRef}
      onOpenDetails={onOpenDetails}
      onSwitch={onSwitch}
      onRelogin={onRelogin}
      onRemove={onRemove}
      onOpenContextMenu={onOpenContextMenu}
    />
  )
})
