import { memo, useCallback, useId, useState } from 'react'
import { Check, CircleAlert, CircleCheck, Copy, Loader2 } from 'lucide-react'
import type { Account } from '../../types'
import type { AccountAttention } from '../../utils/accountFeedback'
import { readableStatusError } from '../../utils/dateUtils'
import { quotaIsStale } from '../../utils/quotaWindows'
import { useTimeValue } from '../../hooks/useSharedTicker'
import { usePrivacy } from '../../context/usePrivacy'

type AccountStatusProps = {
  account: Account
  attention: AccountAttention | null
  isRefreshing: boolean
  actionDisabled: boolean
  onRefreshQuota: (id: string) => Promise<void>
  onDetectSubscription: (id: string) => Promise<void>
  onRelogin: (account: Account) => void
}

export const AccountStatus = memo(function AccountStatus({
  account,
  attention,
  isRefreshing,
  actionDisabled,
  onRefreshQuota,
  onDetectSubscription,
  onRelogin
}: AccountStatusProps) {
  const [copied, setCopied] = useState(false)
  const statusId = useId()
  const { privacyMode } = usePrivacy()
  const isStale = useTimeValue(
    useCallback(() => quotaIsStale(account.quota), [account.quota]),
    Boolean(account.quota) && !attention
  )

  if (attention || isStale || (!account.quota && !isRefreshing)) {
    const kind = attention?.kind
    const rawMessage = attention?.message
    const label = kind === 'verify' ? 'Verify in Antigravity'
      : kind === 'relogin' ? 'Sign-in required'
      : kind === 'subscription' ? 'Plan refresh failed'
      : kind === 'quota' ? 'Quota refresh failed'
      : kind === 'notice' ? 'Refresh warning'
      : isStale ? 'Saved quota' : 'No quota yet'
    const action = kind === 'relogin' ? 'Re-login'
      : kind === 'subscription' ? 'Retry plan'
      : attention ? 'Retry quota' : 'Refresh quota'
    const accountLabel = privacyMode ? 'account' : (account.email ?? 'account')

    return (
      <div className={`account-status-feedback${attention ? ' account-status-warning' : ' text-ag-muted'}`}>
        <div className="account-status-line" title={readableStatusError(rawMessage) ?? label}>
          {attention && <CircleAlert size={13} className="account-status-icon" aria-hidden="true" />}
          <span id={statusId}>{label}</span>
          {rawMessage && (
            <button
              type="button"
              className="account-status-copy"
              aria-label={copied ? 'Copied' : 'Copy status details'}
              title={copied ? 'Copied' : 'Copy status details'}
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(rawMessage)
                  setCopied(true)
                  setTimeout(() => setCopied(false), 2000)
                } catch {
                  // The browser can deny clipboard access.
                }
              }}
            >
              {copied ? <Check size={12} aria-hidden="true" /> : <Copy size={12} aria-hidden="true" />}
            </button>
          )}
        </div>
        <button
          type="button"
          className="account-status-action"
          aria-label={`${action} for ${accountLabel}`}
          aria-describedby={kind === 'verify' ? statusId : undefined}
          aria-disabled={actionDisabled}
          onClick={() => {
            if (actionDisabled) return
            if (kind === 'relogin') onRelogin(account)
            else if (kind === 'subscription') void onDetectSubscription(account.id)
            else void onRefreshQuota(account.id)
          }}
        >
          {action}
        </button>
      </div>
    )
  }

  if (isRefreshing) {
    return (
      <span className="account-status account-status-loading" title="Checking status and quotas…">
        <Loader2 size={13} className="account-status-icon animate-spin" aria-hidden="true" />
        <span>Checking…</span>
      </span>
    )
  }

  return (
    <span className="account-status account-status-healthy">
      <CircleCheck size={14} className="account-status-icon" aria-hidden="true" />
      <span>Healthy</span>
    </span>
  )
})
