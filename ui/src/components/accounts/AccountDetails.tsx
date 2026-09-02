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
import { usePrivacy } from '../../context/PrivacyContext.tsx'

type AccountDetailsProps = {
  account: Account
  isActive: boolean
  isRecommended: boolean
  busyKeys: ReadonlySet<string>
  refreshingAll: boolean
  autoRefreshing: boolean
  onBack: () => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRefreshQuota: (accountId: string) => Promise<void>
  onDetectSubscription: (accountId: string) => Promise<void>
}

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short'
})

function formatDateTime(timestamp: number | null | undefined): string {
  return timestamp ? dateTimeFormatter.format(new Date(timestamp * 1000)) : 'Not reported'
}

function statusDetails(account: Account, isRefreshing = false): { label: string; tone: string; message: string } {
  if (account.tokenHealth.status === 'needs_relogin') {
    return {
      label: 'Re-login required',
      tone: 'danger',
      message: readableStatusError(account.tokenHealth.lastError) ?? 'The saved session can no longer be refreshed.'
    }
  }
  if (isRefreshing || account.tokenHealth?.status === 'unknown' || (!account.tokenHealth?.lastCheckedAt && !account.quota)) {
    return {
      label: 'Checking status…',
      tone: 'loading',
      message: 'Checking authentication health and latest quota limits…'
    }
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
  refreshingAll,
  autoRefreshing,
  onBack,
  onSwitch,
  onRelogin,
  onRefreshQuota,
  onDetectSubscription
}: AccountDetailsProps) {
  const { privacyMode, maskEmail, maskAccountId } = usePrivacy()
  const headingRef = useRef<HTMLHeadingElement>(null)
  const [copiedId, setCopiedId] = useState(false)
  const quotaColumns = useMemo(() => quotaColumnsForAccounts([account]), [account])
  const refreshingQuota = busyKeys.has(`quota:${account.id}`)
  const detectingSubscription = busyKeys.has(`subscription-detect:${account.id}`)
  const isRefreshingAccount =
    refreshingAll ||
    autoRefreshing ||
    refreshingQuota ||
    detectingSubscription ||
    busyKeys.has('refresh') ||
    busyKeys.has('refresh-all') ||
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
              <div>
                <dt>Account ID</dt>
                <dd className="allow-select flex items-center justify-between gap-1.5">
                  <span className={`truncate ${privacyMode ? 'privacy-masked' : ''}`}>
                    {privacyMode ? maskAccountId(account.accountId) : (account.accountId ?? 'Not reported')}
                  </span>
                  {account.accountId && (
                    <button
                      type="button"
                      onClick={() => void copyAccountId()}
                      className="text-ag-muted hover:text-white transition-colors p-0.5"
                      title={copiedId ? 'Copied!' : 'Copy Account ID'}
                      aria-label="Copy Account ID"
                    >
                      {copiedId ? <Check size={13} className="text-green-400" /> : <Copy size={13} />}
                    </button>
                  )}
                </dd>
              </div>
              <div><dt>First login</dt><dd>{formatDateTime(account.createdAt)}</dd></div>
              <div><dt>Last login</dt><dd>{formatDateTime(account.lastLoginAt)}</dd></div>
              <div><dt>Quota updated</dt><dd>{formatDateTime(account.quota?.fetchedAt)}</dd></div>
              <div><dt>Token checked</dt><dd>{formatDateTime(account.tokenHealth.lastCheckedAt)}</dd></div>
            </dl>
          </section>

          {status.tone !== 'healthy' && (
            <section className={`account-detail-sidebar-section account-detail-health account-detail-health-${status.tone}`}>
              <h2>Connection status</h2>
              <p className="allow-select">{status.message}</p>
            </section>
          )}
        </aside>
      </div>
    </article>
  )
}
