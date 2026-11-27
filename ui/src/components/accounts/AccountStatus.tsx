import { memo, useId, useState } from 'react'
import { Check, CircleAlert, CircleCheck, Copy, Loader2 } from 'lucide-react'
import type { Account } from '../../types'
import { recordedAccountStatus, type AccountAttention } from '../../utils/accountFeedback'
import { readableStatusError } from '../../utils/dateUtils'
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

  if (attention) {
    const { kind, message: rawMessage } = attention
    const label = kind === 'verify' ? 'Verify in Antigravity'
      : kind === 'relogin' ? 'Sign-in required'
      : kind === 'subscription' ? 'Plan refresh failed'
      : kind === 'quota' ? 'Quota refresh failed'
      : 'Refresh warning'
    const action = kind === 'relogin' ? 'Re-login'
      : kind === 'subscription' ? 'Retry plan'
      : 'Retry quota'
    const accountLabel = privacyMode ? 'account' : (account.email ?? 'account')

    return (
      <div className="account-status-feedback account-status-warning">
        <div className="account-status-line" title={readableStatusError(rawMessage) ?? label}>
          <CircleAlert size={13} className="account-status-icon" aria-hidden="true" />
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

  const status = recordedAccountStatus(account)
  const healthy = status.tone === 'healthy'
  return (
    <span className={`account-status ${healthy ? 'account-status-healthy' : 'text-ag-muted'}`} title={status.message}>
      {healthy && <CircleCheck size={14} className="account-status-icon" aria-hidden="true" />}
      {status.label}
    </span>
  )
})
