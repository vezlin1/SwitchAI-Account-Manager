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
import type { Account, Recommendation } from '../../types'
import { formatSubscriptionPlan, readableStatusError } from '../../utils/dateUtils'
import { accountAttention, quotaUpdateBlocked, recordedAccountStatus, type AccountAttention, type AccountRefreshNotice } from '../../utils/accountFeedback'
import { QuotaCell } from './QuotaCell'
import { detailQuotaGroups } from '../../utils/quotaWindows'
import { SubscriptionDateControl } from './SubscriptionDateControl'
import { EventTime } from './EventTime'
import { usePrivacy } from '../../context/usePrivacy'

type AccountDetailsProps = {
  account: Account
  refreshNotice?: AccountRefreshNotice
  isActive: boolean
  recommendation: Recommendation | null
  busyKeys: ReadonlySet<string>
  onBack: () => void
  onEditTags: (account: Account) => void
  onSwitch: (account: Account) => void
  onRelogin: (account: Account) => void
  onRefreshQuota: (accountId: string) => Promise<void>
  onDetectSubscription: (accountId: string) => Promise<void>
}

function recommendationLabel(recommendation: Recommendation): string {
  switch (recommendation.reason) {
    case 'resets_soon': return 'Best right now · resets soon'
    case 'larger_plan': return 'Best right now · bigger plan'
    default: return 'Best right now'
  }
}

function recommendationTitle(recommendation: Recommendation): string {
  const remaining = `${recommendation.projected ? '~' : ''}${Math.round(recommendation.remainingPercent)}% left`
  const why = recommendation.reason === 'resets_soon'
    ? 'Its long quota window resets within a day, so spending it first loses nothing.'
    : recommendation.reason === 'larger_plan'
      ? 'A lower percentage of a larger plan is more absolute quota.'
      : 'It has the most quota left.'
  const projected = recommendation.projected
    ? ' A limit has reset since the last check; the new value is not confirmed yet.'
    : ''
  return `${remaining}. ${why}${projected}`
}

function statusDetails(
  account: Account, attention: AccountAttention | null, isRefreshing: boolean
): { label: string; tone: 'healthy' | 'loading' | 'warning' | 'neutral'; message: string } {
  if (attention?.kind === 'verify') {
    return {
      label: 'Verification required',
      tone: 'warning',
      message: 'Open Antigravity with this Google account, complete verification, then retry the quota check.'
    }
  }
  if (attention) {
    const label = attention.kind === 'relogin' ? 'Signed out'
      : attention.kind === 'subscription' ? 'Couldn’t check the plan'
      : attention.kind === 'notice' ? 'Refresh warning'
      : 'Couldn’t check limits'
    return {
      label,
      tone: 'warning',
      message: readableStatusError(attention.message) ?? label
    }
  }
  if (isRefreshing) {
    return {
      label: 'Checking status…',
      tone: 'loading',
      message: 'Checking authentication health and latest quota limits…'
    }
  }
  return recordedAccountStatus(account)
}

export function AccountDetails({
  account,
  refreshNotice,
  isActive,
  recommendation,
  busyKeys,
  onBack,
  onEditTags,
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
  const quotaGroups = useMemo(() => detailQuotaGroups(account), [account])
  const refreshingQuota = busyKeys.has(`quota:${account.id}`)
  const detectingSubscription = busyKeys.has(`subscription-detect:${account.id}`)
  const isRefreshingAccount =
    refreshingQuota ||
    detectingSubscription ||
    busyKeys.has(`relogin:${account.id}`) ||
    busyKeys.has(`account:${account.id}:quota`) ||
    busyKeys.has(`account:${account.id}:subscription`)
  const attention = accountAttention(account, refreshNotice)
  const updateBlocked = quotaUpdateBlocked(attention)
  const status = statusDetails(account, attention, isRefreshingAccount)
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
    const message = attention?.message ?? status.message
    if (!message) return
    try {
      await navigator.clipboard.writeText(message)
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
                {account.provider === 'gemini' ? 'In use in Antigravity' : 'In use in Codex'}
              </span>
            )}
            {recommendation && !updateBlocked && (
              <span
                className="account-detail-badge account-detail-badge-recommended"
                title={recommendationTitle(recommendation)}
              >
                <Sparkles size={12} aria-hidden="true" /> {recommendationLabel(recommendation)}
              </span>
            )}
          </div>
          <p>{plan}</p>
        </div>

        <div className="account-details-actions">
          <button type="button" className="account-detail-action" onClick={() => onEditTags(account)}>Edit tags</button>
          {updateBlocked && (
            <button
              type="button"
              className="account-detail-action"
              onClick={() => void onRefreshQuota(account.id)}
              disabled={busy}
            >
              {refreshingQuota ? <Loader2 size={16} className="animate-spin" /> : <RefreshCw size={16} />}
              Check limits again
            </button>
          )}
          {account.tokenHealth.status === 'needs_relogin' ? (
            <button
              type="button"
              className="account-detail-action account-detail-action-warning"
              onClick={() => onRelogin(account)}
              disabled={busy}
            >
              <KeyRound size={16} /> Sign in again
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
              <span className={`account-detail-status account-detail-status-${status.tone}`} title={status.message}>
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

            {quotaGroups.length > 0 ? (
              <div className="account-detail-quota-list">
                {quotaGroups.map(({ title, cells }) => (
                  <div key={title ?? 'limits'} className="account-detail-quota-list">
                    {title && <h3 className="text-sm font-semibold">{title}</h3>}
                    {cells.map(({ column, window }) => (
                      <div className="account-detail-quota-row" key={column.key}>
                        <QuotaCell
                          window={window}
                          fetchedAt={account.quota?.fetchedAt ?? null}
                          updateBlocked={updateBlocked}
                          title={column.cellTitle}
                          isRefreshing={isRefreshingAccount}
                        />
                      </div>
                    ))}
                  </div>
                ))}
              </div>
            ) : (
              <div className="account-detail-empty">
                <BarChart3 size={18} aria-hidden="true" />
                No quota data available.
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
              <div><dt>Quota updated</dt><dd><EventTime timestamp={account.quota?.fetchedAt} /></dd></div>
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
