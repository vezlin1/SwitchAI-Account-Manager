import { memo, useState } from 'react'
import { Check, CircleAlert, CircleCheck, Copy, Loader2 } from 'lucide-react'
import type { Account } from '../../types'
import { readableStatusError, STATUS_PREVIEW_LIMIT } from '../../utils/dateUtils'

type AccountStatusProps = {
  account: Account
  isRefreshing?: boolean
}

export const AccountStatus = memo(function AccountStatus({
  account,
  isRefreshing = false
}: AccountStatusProps) {
  const [copied, setCopied] = useState(false)
  const tokenHealth = account.tokenHealth
  const isUnknown = tokenHealth?.status === 'unknown'
  const notCheckedYet = !tokenHealth?.lastCheckedAt && !account.quota
  const isChecking = isRefreshing || isUnknown || notCheckedYet

  const needsRelogin = tokenHealth?.status === 'needs_relogin'
  const tokenWarning = tokenHealth?.status === 'network_error' || tokenHealth?.status === 'server_error'
  const quotaIssue = account.issues?.quota ?? null
  const rawStatusError = tokenHealth?.lastError ?? quotaIssue
  const statusTitle = rawStatusError ?? (isChecking ? 'Checking status and quotas…' : 'Healthy')
  const tokenStatusMessage = readableStatusError(tokenHealth?.lastError)
  const accountStatusMessage = readableStatusError(quotaIssue)

  const copyError = async (e: React.MouseEvent) => {
    e.stopPropagation()
    if (!rawStatusError) return
    try {
      await navigator.clipboard.writeText(rawStatusError)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // Ignore
    }
  }

  if (isChecking && !needsRelogin) {
    return (
      <span className="account-status account-status-loading" title={statusTitle}>
        <Loader2 size={13} className="account-status-icon animate-spin text-blue-400" aria-hidden="true" />
        <span className="text-blue-400/90 font-medium">checking…</span>
      </span>
    )
  }

  if (needsRelogin) {
    return (
      <span className="account-status account-status-danger" title={statusTitle}>
        <CircleAlert size={14} className="account-status-icon" aria-hidden="true" />
        <span>{tokenStatusMessage ?? 'needs re-login'}</span>
        {rawStatusError && (
          <button
            type="button"
            onClick={copyError}
            className="ml-1 p-0.5 text-red-300/80 hover:text-white rounded hover:bg-white/10 transition-colors cursor-pointer"
            title={copied ? 'Copied error!' : 'Copy full error'}
          >
            {copied ? <Check size={11} className="text-green-400" /> : <Copy size={11} />}
          </button>
        )}
      </span>
    )
  }

  if (quotaIssue) {
    return (
      <span className="account-status account-status-danger" title={quotaIssue}>
        <CircleAlert size={14} className="account-status-icon" aria-hidden="true" />
        <span>{accountStatusMessage ?? quotaIssue.slice(0, STATUS_PREVIEW_LIMIT)}</span>
        <button
          type="button"
          onClick={copyError}
          className="ml-1 p-0.5 text-red-300/80 hover:text-white rounded hover:bg-white/10 transition-colors cursor-pointer"
          title={copied ? 'Copied error!' : 'Copy full error'}
        >
          {copied ? <Check size={11} className="text-green-400" /> : <Copy size={11} />}
        </button>
      </span>
    )
  }

  if (tokenWarning) {
    return (
      <span className="account-status account-status-warning" title={statusTitle}>
        <CircleAlert size={14} className="account-status-icon" aria-hidden="true" />
        <span>{tokenStatusMessage ?? 'token check warning'}</span>
        {rawStatusError && (
          <button
            type="button"
            onClick={copyError}
            className="ml-1 p-0.5 text-amber-300/80 hover:text-white rounded hover:bg-white/10 transition-colors cursor-pointer"
            title={copied ? 'Copied error!' : 'Copy full error'}
          >
            {copied ? <Check size={11} className="text-green-400" /> : <Copy size={11} />}
          </button>
        )}
      </span>
    )
  }

  return (
    <span className="account-status account-status-healthy">
      <CircleCheck size={14} className="account-status-icon" aria-hidden="true" />
      <span>healthy</span>
    </span>
  )
})
