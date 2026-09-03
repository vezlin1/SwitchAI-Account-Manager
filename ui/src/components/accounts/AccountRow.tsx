import type { DraggableAttributes, DraggableSyntheticListeners } from '@dnd-kit/core'
import { memo, type CSSProperties, type MouseEvent as ReactMouseEvent } from 'react'
import { ChevronRight, EyeOff, GripVertical, KeyRound, Loader2, Trash2, UserCheck } from 'lucide-react'
import type { Account } from '../../types'
import { AccountStatus } from './AccountStatus'
import { QuotaCell } from './QuotaCell'
import { quotaWindowForColumn, type QuotaColumn } from '../../utils/quotaWindows'
import { SubscriptionDateControl } from './SubscriptionDateControl'
import { usePrivacy } from '../../context/usePrivacy'

import { useAccountRowState } from './useAccountRowState'

type AccountRowProps = {
  account: Account
  isActive: boolean
  isSwitching: boolean
  isRemoving: boolean
  isRelogining: boolean
  isRefreshing: boolean
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
  isSwitching,
  isRemoving,
  isRelogining,
  isRefreshing,
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
  const { privacyMode, maskEmail } = usePrivacy()
  const quota = account.quota
  const {
    isSwitching: switching,
    isRemoving: removing,
    isRelogining: relogining,
    isRefreshing: isAccountRefreshing,
    needsRelogin,
    detectedSubscriptionDate
  } = useAccountRowState(account, { isSwitching, isRemoving, isRelogining, isRefreshing })

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
          aria-label={`Reorder ${privacyMode ? 'account' : (account.email ?? 'account')}`}
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
          aria-label={privacyMode ? 'Open details for account' : `Open details for ${account.email ?? 'account'}`}
        >
          <div className="account-identity-copy min-w-0 flex-1 flex flex-col gap-0.5">
            <div className="flex items-center gap-2 flex-wrap">
              <span
                className={`allow-select account-identity-email font-semibold text-xs text-ag-text truncate ${privacyMode ? 'privacy-masked' : ''}`}
                title={privacyMode ? 'Sensitive data hidden (Privacy Mode)' : (account.email ?? 'Unknown email')}
              >
                {privacyMode ? maskEmail(account.email) : (account.email ?? 'Unknown email')}
              </span>
              {isActive && (
                <span className="account-active-badge">
                  Active
                </span>
              )}
            </div>
            {hiddenFromAll && (
              <span className="account-hidden-badge">
                <EyeOff size={10} aria-hidden="true" /> Hidden
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
            disabled={isActive || needsRelogin || switching || isAccountRefreshing || orderBusy}
            title={
              isActive
                ? (account.provider === 'gemini' ? 'Currently active in Antigravity / Gemini' : 'Currently active in Codex / ChatGPT')
                : needsRelogin
                  ? 'Re-login required before switching'
                  : (account.provider === 'gemini' ? 'Switch active Gemini account' : 'Switch active ChatGPT account')
            }
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
              disabled={relogining || switching || removing || isAccountRefreshing || orderBusy}
              title="Re-login account"
              aria-label={`Re-login ${account.email ?? 'account'}`}
            >
              {relogining
                ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
                : <KeyRound size={14} aria-hidden="true" />}
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
