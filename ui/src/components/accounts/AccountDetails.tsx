import { useEffect, useMemo, useRef, useState } from 'react'
import {
  ArrowLeft,
  BarChart3,
  Check,
  CircleAlert,
  CircleCheck,
  Copy,
  KeyRound,
  Loader2,
  RefreshCw,
  Sparkles,
  UserCheck
} from 'lucide-react'
import type { Account } from '../../types'
import { formatSubscriptionPlan, readableStatusError } from '../../utils/dateUtils'
import { QuotaCell } from './QuotaCell'
import { quotaColumnsForAccounts, quotaWindowForColumn } from '../../utils/quotaWindows'
import { SubscriptionDateControl } from './SubscriptionDateControl'
import { EventTime } from './EventTime'
import { QuotaFreshness } from './QuotaFreshness'
import { usePrivacy } from '../../context/usePrivacy'

type AccountDetailsProps = {
  account: Account
  isActive: boolean
  isRecommended: boolean
  busyKeys: ReadonlySet<string>
  onBack: () => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRefreshQuota: (accountId: string) => Promise<void>
  onDetectSubscription: (accountId: string) => Promise<void>
}

function statusDetails(account: Account, isRefreshing = false): { label: string; tone: string; message: string } {
  if (account.tokenHealth.status === 'needs_relogin') {
    return {
      label: 'Re-login required',
      tone: 'danger',
      message: readableStatusError(account.tokenHealth.lastError) ?? 'The saved session can no longer be refreshed.'
    }
  }
  if (isRefreshing) {
    return {
      label: 'Checking status…',
      tone: 'loading',
      message: 'Checking authentication health and latest quota limits…'
    }
  }
  if (account.tokenHealth?.status === 'unknown' || (!account.tokenHealth?.lastCheckedAt && !account.quota)) {
    return { label: 'Not checked', tone: 'warning', message: 'Refresh this account to check its current status and limits.' }
  }
  const quotaIssue = readableStatusError(account.issues?.quota)
  if (quotaIssue) {
    return { label: 'Quota refresh error', tone: 'danger', message: quotaIssue }
  }
  const subscriptionIssue = readableStatusError(account.issues?.subscription)
  if (subscriptionIssue) {
    return {
      label: 'Subscription refresh error',
      tone: 'warning',
      message: subscriptionIssue
    }
  }
  if (account.tokenHealth.status === 'network_error' || account.tokenHealth.status === 'server_error') {
    return {
      label: 'Connection warning',
      tone: 'warning',
      message: readableStatusError(account.tokenHealth.lastError) ?? 'The last background check could not be completed.'
    }
  }
  return {
    label: 'Healthy',
    tone: 'healthy',
    message: account.provider === 'gemini'
      ? 'Google authentication and Antigravity limit checks are working normally.'
      : 'Authentication and quota checks are working normally.'
  }
}

export function AccountDetails({
  account,
  isActive,
  isRecommended,
  busyKeys,
  onBack,
  onSwitch,
  onRelogin,
  onRefreshQuota,
  onDetectSubscription
}: AccountDetailsProps) {
  const { privacyMode, maskEmail, maskAccountId } = usePrivacy()
  const headingRef = useRef<HTMLHeadingElement>(null)
  const [copiedId, setCopiedId] = useState(false)
  const [copiedEmail, setCopiedEmail] = useState(false)
  const [copiedStatus, setCopiedStatus] = useState(false)
  const quotaColumns = useMemo(() => quotaColumnsForAccounts([account]), [account])
  const refreshingQuota = busyKeys.has(`quota:${account.id}`)
  const detectingSubscription = busyKeys.has(`subscription-detect:${account.id}`)
  const isRefreshingAccount =
    refreshingQuota ||
    detectingSubscription ||
    busyKeys.has(`relogin:${account.id}`) ||
    busyKeys.has(`account:${account.id}:quota`) ||
    busyKeys.has(`account:${account.id}:subscription`)
  const status = statusDetails(account, isRefreshingAccount)
  const busy = isRefreshingAccount
    || busyKeys.has(`switch:${account.id}`)
    || busyKeys.has(`delete:${account.id}`)
  const plan = formatSubscriptionPlan(
    account.subscriptionPlan ?? account.quota?.planType,
    account.provider
  ) ?? 'Plan not reported'
  const isGemini = account.provider === 'gemini'

  useEffect(() => {
    headingRef.current?.focus()
  }, [account.id])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (e.defaultPrevented || document.querySelector('[aria-modal="true"]')) return
        e.preventDefault()
        onBack()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onBack])

  const copyAccountId = async () => {
    if (!account.accountId) return
    try {
      await navigator.clipboard.writeText(account.accountId)
      setCopiedId(true)
      setTimeout(() => setCopiedId(false), 2000)
    } catch {
      // Ignore clipboard write failures
    }
  }

  const copyEmail = async () => {
    if (!account.email) return
    try {
      await navigator.clipboard.writeText(account.email)
      setCopiedEmail(true)
      setTimeout(() => setCopiedEmail(false), 2000)
    } catch {
      // Ignore clipboard write failures
    }
  }

  const copyStatusMessage = async () => {
    if (!status.message) return
    try {
      await navigator.clipboard.writeText(status.message)
      setCopiedStatus(true)
      setTimeout(() => setCopiedStatus(false), 2000)
    } catch {
      // Ignore clipboard write failures
    }
  }

  return (
    <article className="account-details page-fade" aria-labelledby="account-details-title">
      <header className="account-details-header">
        <button type="button" className="account-details-back" onClick={onBack}>
          <ArrowLeft size={17} aria-hidden="true" />
          <span>All accounts</span>
        </button>

        <div className="account-details-title-group">
          <div className="account-details-title-line">
            <h2
              id="account-details-title"
              ref={headingRef}
              tabIndex={-1}
              className={privacyMode ? 'privacy-masked' : ''}
              title={privacyMode ? 'Sensitive data hidden (Privacy Mode)' : (account.email ?? 'Unnamed account')}
            >
              {privacyMode ? maskEmail(account.email) : (account.email ?? 'Unnamed account')}
            </h2>
            {account.email && (
              <button
                type="button"
                onClick={() => void copyEmail()}
                className="inline-flex items-center justify-center w-6 h-6 rounded-md text-ag-muted hover:text-white hover:bg-white/[0.08] active:scale-90 transition-all cursor-pointer select-none"
                title={copiedEmail ? 'Copied!' : 'Copy email'}
                aria-label="Copy email"
              >
                {copiedEmail ? <Check size={13} className="text-green-400" /> : <Copy size={13} />}
              </button>
            )}
            {isActive && (
              <span className="account-detail-badge">
                {account.provider === 'gemini' ? 'Active in Antigravity' : 'Active in Codex'}
              </span>
            )}
            {isRecommended && (
              <span className="account-detail-badge account-detail-badge-recommended">
                <Sparkles size={12} aria-hidden="true" /> Best reserve
              </span>
            )}
          </div>
          <p>{plan}</p>
        </div>

        <div className="account-details-actions">
          <button
            type="button"
            className="account-detail-action"
            onClick={() => void onRefreshQuota(account.id)}
            disabled={busy}
          >
            {refreshingQuota ? <Loader2 size={16} className="animate-spin" /> : <RefreshCw size={16} />}
            Refresh quota
          </button>
          {account.tokenHealth.status === 'needs_relogin' ? (
            <button
              type="button"
              className="account-detail-action account-detail-action-warning"
              onClick={() => onRelogin(account)}
              disabled={busy}
            >
              <KeyRound size={16} /> Re-login
            </button>
          ) : (
            <button
              type="button"
              className="account-detail-action account-detail-action-primary"
              onClick={() => onSwitch(account)}
              disabled={isActive || busy}
            >
              <UserCheck size={16} /> {isActive ? 'Active account' : 'Use account'}
            </button>
          )}
        </div>
      </header>

      <div className="account-details-body">
        <main className="account-details-main">
          <section className="account-detail-section" aria-labelledby="account-quota-title">
            <div className="account-detail-section-heading">
              <h2 id="account-quota-title">{isGemini ? 'Antigravity limits' : 'ChatGPT limits'}</h2>
              <span className={`account-detail-status account-detail-status-${status.tone}`}>
                {status.tone === 'healthy' ? (
                  <CircleCheck size={15} aria-hidden="true" />
                ) : status.tone === 'loading' ? (
                  <Loader2 size={15} className="animate-spin text-blue-400" aria-hidden="true" />
                ) : (
                  <CircleAlert size={15} aria-hidden="true" />
                )}
                {status.label}
              </span>
            </div>

            {quotaColumns.length > 0 ? (
              <div className="account-detail-quota-list">
                {quotaColumns.map((column) => {
                  const window = quotaWindowForColumn(account.quota, column)
                  return (
                    <div className="account-detail-quota-row" key={column.key}>
                      <QuotaCell
                        value={window?.usedPercent}
                        resetAt={window?.resetAt}
                        title={column.cellTitle}
                        isRefreshing={isRefreshingAccount}
                      />
                    </div>
                  )
                })}
              </div>
            ) : (
              <div className="account-detail-empty">
                <BarChart3 size={18} aria-hidden="true" />
                {isGemini
                  ? 'Refresh this account to load the limits Google currently exposes for Antigravity.'
                  : 'Refresh this account to collect its first quota value.'}
              </div>
            )}
          </section>
        </main>

        <aside className="account-details-sidebar" aria-label="Account information">
          <section className="account-detail-sidebar-section">
            <h2>{isGemini ? 'Google plan' : 'Subscription'}</h2>
            <p className="account-detail-plan">{plan}</p>
            {!isGemini && (
              <SubscriptionDateControl
                key={`${account.id}:${account.subscriptionExpiresAt ?? 'empty'}`}
                value={account.subscriptionDetectedAt ? account.subscriptionExpiresAt : null}
                plan={null}
                hideNoPlanBadge={true}
              />
            )}
            <button
              type="button"
              className="account-subscription-refresh"
              onClick={() => void onDetectSubscription(account.id)}
              disabled={busy}
            >
              {detectingSubscription
                ? <Loader2 size={14} className="animate-spin" aria-hidden="true" />
                : <RefreshCw size={14} aria-hidden="true" />}
              {isGemini ? 'Refresh plan' : 'Refresh subscription'}
            </button>
          </section>

          <section className="account-detail-sidebar-section">
            <h2>Account information</h2>
            <dl className="account-detail-metadata">
              <div className="account-detail-id-row">
                <dt>Account ID</dt>
                <dd className="account-detail-id-value">
                  {account.accountId ? (
                    <button
                      type="button"
                      onClick={() => void copyAccountId()}
                      className={`account-detail-copy-field${copiedId ? ' account-detail-copy-field-copied' : ''}`}
                      title={privacyMode ? 'Copy account ID' : account.accountId}
                      aria-label={copiedId ? 'Account ID copied' : 'Copy account ID'}
                    >
                      <span className={`account-detail-id-text ${privacyMode && !copiedId ? 'privacy-masked' : ''}`}>
                        {copiedId ? 'Copied to clipboard' : privacyMode ? maskAccountId(account.accountId) : account.accountId}
                      </span>
                      <span className="account-detail-copy-icon" aria-hidden="true">
                        {copiedId ? <Check size={14} /> : <Copy size={14} />}
                      </span>
                    </button>
                  ) : (
                    <span className="text-ag-muted">Not reported</span>
                  )}
                  <span className="visually-hidden" role="status">{copiedId ? 'Account ID copied to clipboard' : ''}</span>
                </dd>
              </div>
              <div><dt>First login</dt><dd><EventTime timestamp={account.createdAt} /></dd></div>
              <div><dt>Last login</dt><dd><EventTime timestamp={account.lastLoginAt} /></dd></div>
              <div><dt>Quota updated</dt><dd><QuotaFreshness account={account} prefix={false} /></dd></div>
            </dl>
          </section>

          {status.tone !== 'healthy' && (
            <section className={`account-detail-sidebar-section account-detail-health account-detail-health-${status.tone}`}>
              <div className="flex items-center justify-between gap-2 mb-1">
                <h2>Connection status</h2>
                {status.message && (
                  <button
                    type="button"
                    onClick={() => void copyStatusMessage()}
                    className="inline-flex items-center gap-1 text-[11px] text-ag-muted hover:text-white transition-colors cursor-pointer"
                    title={copiedStatus ? 'Copied!' : 'Copy error message'}
                  >
                    {copiedStatus ? <Check size={12} className="text-green-400" /> : <Copy size={12} />}
                    <span>{copiedStatus ? 'Copied' : 'Copy'}</span>
                  </button>
                )}
              </div>
              <p className="allow-select">{status.message}</p>
            </section>
          )}
        </aside>
      </div>
    </article>
  )
}
