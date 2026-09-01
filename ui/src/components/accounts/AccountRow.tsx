import type { DraggableAttributes, DraggableSyntheticListeners } from '@dnd-kit/core'
import { memo, type CSSProperties, type MouseEvent as ReactMouseEvent } from 'react'
import { ChevronRight, EyeOff, GripVertical, KeyRound, Loader2, Trash2, UserCheck } from 'lucide-react'
import type { Account } from '../../types'
import { AccountStatus } from './AccountStatus'
import { QuotaCell } from './QuotaCell'
import { quotaWindowForColumn, type QuotaColumn } from '../../utils/quotaWindows'
import { SubscriptionDateControl } from './SubscriptionDateControl'

type AccountRowProps = {
  account: Account
  isActive: boolean
  busyKeys: ReadonlySet<string>
  refreshingAll: boolean
  autoRefreshing: boolean
  quotaColumns: QuotaColumn[]
  hiddenFromAll: boolean
  orderBusy?: boolean
  rowStyle?: CSSProperties
  rowRef?: (element: HTMLTableRowElement | null) => void
  dragHandleAttributes?: DraggableAttributes
  dragHandleListeners?: DraggableSyntheticListeners
  setDragHandleRef?: (element: HTMLElement | null) => void
  onOpenDetails: (account: Account, opener: HTMLElement) => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRemove: (account: Account) => void
  onOpenContextMenu?: (event: ReactMouseEvent<HTMLTableRowElement>, account: Account) => void
}

export const AccountRow = memo(function AccountRow({
  account,
  isActive,
  busyKeys,
  refreshingAll,
  autoRefreshing,
  quotaColumns,
  hiddenFromAll,
  orderBusy = false,
  rowStyle,
  rowRef,
  dragHandleAttributes,
  dragHandleListeners,
  setDragHandleRef,
  onOpenDetails,
  onSwitch,
  onRelogin,
  onRemove,
  onOpenContextMenu
}: AccountRowProps) {
  const quota = account.quota
  const switching = busyKeys.has(`switch:${account.id}`)
  const removing = busyKeys.has(`delete:${account.id}`)
  const needsRelogin = account.tokenHealth?.status === 'needs_relogin'
  const detectedSubscriptionDate = account.subscriptionDetectedAt ? account.subscriptionExpiresAt : null

  const isAccountRefreshing =
    refreshingAll ||
    autoRefreshing ||
    busyKeys.has('refresh') ||
    busyKeys.has(`quota:${account.id}`) ||
    busyKeys.has(`subscription-detect:${account.id}`) ||
    busyKeys.has(`relogin:${account.id}`) ||
    busyKeys.has(`account:${account.id}:quota`) ||
    busyKeys.has(`account:${account.id}:subscription`)

  return (
    <tr
      ref={rowRef}
      className={`account-row group${isActive ? ' account-row-active' : ''}${isAccountRefreshing ? ' account-row-refreshing' : ''}`}
      style={rowStyle}
      onContextMenu={onOpenContextMenu ? (event) => onOpenContextMenu(event, account) : undefined}
    >
      <td className="account-row-order" data-label="Order">
        <button
          type="button"
          ref={setDragHandleRef}
          className={`account-drag-handle${orderBusy ? ' account-drag-handle-disabled' : ''}`}
          title="Drag to reorder"
          aria-label={`Reorder ${account.email ?? 'account'}`}
          disabled={orderBusy}
          {...dragHandleAttributes}
          {...dragHandleListeners}
        >
          <GripVertical size={14} aria-hidden="true" />
        </button>
      </td>
      <th scope="row" className="account-row-identity" data-label="Account">
        <button
          type="button"
          className="account-identity-button flex items-center justify-between w-full text-left"
          onClick={(event) => {
            const selection = window.getSelection()?.toString()
            if (selection && selection.trim().length > 0) return
            onOpenDetails(account, event.currentTarget)
          }}
          aria-label={`Open details for ${account.email ?? 'account'}`}
        >
          <div className="account-identity-copy min-w-0 flex-1 flex flex-col gap-0.5">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="allow-select account-identity-email font-semibold text-xs text-ag-text truncate" title={account.email ?? 'Unknown email'}>
                {account.email ?? 'Unknown email'}
              </span>
              {isActive && (
                <span className="account-active-badge">
                  Active
                </span>
              )}
            </div>
            {hiddenFromAll && (
              <span className="account-hidden-badge">
                <EyeOff size={10} aria-hidden="true" /> Hidden from All
              </span>
            )}
          </div>
          <ChevronRight size={14} className="account-identity-arrow text-ag-muted opacity-40 group-hover:opacity-100 flex-shrink-0" aria-hidden="true" />
        </button>
      </th>
      {quotaColumns.map((column) => {
        const window = quotaWindowForColumn(quota, column)
        return (
          <td key={column.key} className="account-row-quota" data-label={column.label}>
            <QuotaCell
              value={window?.usedPercent}
              resetAt={window?.resetAt}
              title={column.cellTitle}
              isRefreshing={isAccountRefreshing}
            />
          </td>
        )
      })}
      <td className="account-row-status" data-label="Status">
        <AccountStatus account={account} isRefreshing={isAccountRefreshing} />
      </td>
      <td className="account-row-subscription" data-label="Subscription">
        <SubscriptionDateControl
          key={`${account.id}:${detectedSubscriptionDate ?? 'empty'}`}
          value={detectedSubscriptionDate}
          plan={account.subscriptionPlan ?? quota?.planType ?? null}
          provider={account.provider}
        />
      </td>
      <td className="account-row-actions" data-label="Actions">
        <div className="account-actions">
          <button
            type="button"
            className={`account-action${isActive ? ' account-action-active' : ''}`}
            onClick={() => onSwitch(account)}
            disabled={isActive || needsRelogin || switching || refreshingAll || autoRefreshing || orderBusy}
            title={account.provider === 'gemini' ? 'Switch active Antigravity account' : 'Switch active Codex account'}
            aria-label={`Switch to ${account.email ?? 'account'}`}
          >
            {switching
              ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
              : <UserCheck size={14} aria-hidden="true" />}
          </button>

          {needsRelogin && (
            <button
              type="button"
              className="account-action account-action-relogin"
              onClick={() => onRelogin(account)}
              disabled={switching || removing || refreshingAll || autoRefreshing || orderBusy}
              title="Re-login account"
              aria-label={`Re-login ${account.email ?? 'account'}`}
            >
              <KeyRound size={14} aria-hidden="true" />
            </button>
          )}

          <button
            type="button"
            className="account-action account-action-danger"
            onClick={() => onRemove(account)}
            disabled={removing || switching || orderBusy}
            title="Delete account"
            aria-label={`Delete ${account.email ?? 'account'}`}
          >
            {removing
              ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
              : <Trash2 size={14} aria-hidden="true" />}
          </button>
        </div>
      </td>
    </tr>
  )
})
