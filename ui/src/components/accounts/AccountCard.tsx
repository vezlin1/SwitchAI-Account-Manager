import { memo } from 'react'
import { ChevronRight, Eye, EyeOff, KeyRound, Loader2, Trash2, UserCheck } from 'lucide-react'
import type { Account } from '../../types'
import { AccountStatus } from './AccountStatus'
import { QuotaCell } from './QuotaCell'
import { quotaWindowForColumn, type QuotaColumn } from '../../utils/quotaWindows'
import { SubscriptionDateControl } from './SubscriptionDateControl'
import { usePrivacy } from '../../context/usePrivacy'

import { useAccountRowState } from './useAccountRowState'

type AccountCardProps = {
  account: Account
  isActive: boolean
  isSwitching: boolean
  isRemoving: boolean
  isRelogining: boolean
  isRefreshing: boolean
  quotaColumns: QuotaColumn[]
  hiddenFromAll: boolean
  onOpenDetails: (account: Account, opener: HTMLElement) => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRemove: (account: Account) => void
  onToggleInAll: (accountId: string) => void
}

export const AccountCard = memo(function AccountCard({
  account,
  isActive,
  isSwitching,
  isRemoving,
  isRelogining,
  isRefreshing,
  quotaColumns,
  hiddenFromAll,
  onOpenDetails,
  onSwitch,
  onRelogin,
  onRemove,
  onToggleInAll
}: AccountCardProps) {
  const { privacyMode, maskEmail } = usePrivacy()
  const {
    isSwitching: switching,
    isRemoving: removing,
    isRelogining: relogining,
    isRefreshing: isAccountRefreshing,
    needsRelogin,
    detectedSubscriptionDate
  } = useAccountRowState(account, { isSwitching, isRemoving, isRelogining, isRefreshing })

  return (
    <article className={`account-card${isActive ? ' account-card-active' : ''}${isAccountRefreshing ? ' account-card-refreshing' : ''}`}>
      <header className="account-card-header">
        <button
          type="button"
          className="account-card-identity"
          onClick={(event) => onOpenDetails(account, event.currentTarget)}
          aria-label={privacyMode ? 'Open details for account' : `Open details for ${account.email ?? 'account'}`}
        >
          <span className="account-card-identity-copy">
            <span
              className={`allow-select account-card-email ${privacyMode ? 'privacy-masked' : ''}`}
              title={privacyMode ? 'Sensitive data hidden (Privacy Mode)' : (account.email ?? 'Unknown email')}
            >
              {privacyMode ? maskEmail(account.email) : (account.email ?? 'Unknown email')}
            </span>
            <span className="account-card-subline">
              {isActive ? 'Active' : ''}
              {hiddenFromAll ? (isActive ? ' · Hidden' : 'Hidden') : ''}
            </span>
          </span>
          <ChevronRight size={17} className="account-card-arrow" aria-hidden="true" />
        </button>
        <div className="account-card-actions">
          <button
            type="button"
            className={`account-card-icon-action${isActive ? ' account-card-icon-action-active' : ''}`}
            onClick={() => onSwitch(account)}
            disabled={isActive || needsRelogin || switching || isAccountRefreshing}
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
              ? <Loader2 size={16} className="animate-spin" aria-hidden="true" />
              : <UserCheck size={16} aria-hidden="true" />}
          </button>
          {needsRelogin && (
            <button
              type="button"
              className="account-card-icon-action account-card-icon-action-warning"
              onClick={() => onRelogin(account)}
              disabled={relogining || switching || removing || isAccountRefreshing}
              title="Re-login account"
              aria-label={`Re-login ${account.email ?? 'account'}`}
            >
              {relogining
                ? <Loader2 size={16} className="animate-spin" aria-hidden="true" />
                : <KeyRound size={16} aria-hidden="true" />}
            </button>
          )}
          <button
            type="button"
            className="account-card-icon-action"
            onClick={() => onToggleInAll(account.id)}
            aria-pressed={hiddenFromAll}
            title={hiddenFromAll ? 'Unhide' : 'Hide'}
            aria-label={hiddenFromAll ? `Unhide ${account.email ?? 'account'}` : `Hide ${account.email ?? 'account'}`}
          >
            {hiddenFromAll ? <Eye size={16} aria-hidden="true" /> : <EyeOff size={16} aria-hidden="true" />}
          </button>
          <button
            type="button"
            className="account-card-icon-action account-card-icon-action-danger"
            onClick={() => onRemove(account)}
            disabled={removing || switching}
            title="Delete account"
            aria-label={`Delete ${account.email ?? 'account'}`}
          >
            {removing
              ? <Loader2 size={16} className="animate-spin" aria-hidden="true" />
              : <Trash2 size={16} aria-hidden="true" />}
          </button>
        </div>
      </header>

      <div className="account-card-status">
        <span className="account-card-label">Status</span>
        <AccountStatus account={account} isRefreshing={isAccountRefreshing} />
      </div>

      {quotaColumns.length > 0 && (
        <div className="account-card-quota">
          {quotaColumns.map((column) => {
            const window = quotaWindowForColumn(account.quota, column)
            return (
              <QuotaCell
                key={column.key}
                value={window?.usedPercent}
                resetAt={window?.resetAt}
                title={column.cellTitle}
                isRefreshing={isAccountRefreshing}
              />
            )
          })}
        </div>
      )}

      <div className="account-card-subscription">
        <span className="account-card-label">Subscription</span>
        <SubscriptionDateControl
          key={`${account.id}:${detectedSubscriptionDate ?? 'empty'}`}
          value={detectedSubscriptionDate}
          plan={account.subscriptionPlan ?? account.quota?.planType ?? null}
          provider={account.provider}
        />
      </div>
    </article>
  )
})
