import { memo, useCallback } from 'react'
import { Loader2 } from 'lucide-react'
import type { Account, AccountProvider, Recommendation } from '../../types'
import { usePrivacy } from '../../context/usePrivacy'
import { useTimeValue } from '../../hooks/useSharedTicker'
import { formatSubscriptionPlan } from '../../utils/dateUtils'
import { formatTimeUntil } from '../../utils/format'
import { accountBackAt } from '../../utils/quotaWindows'

type BestAccountBarProps = {
  provider: AccountProvider
  accounts: Account[]
  activeAccountId: string | null
  recommendation: Recommendation | null
  switchingAccountId: string | null
  onSwitch: (account: Account) => void
}

const REASONS: Record<Recommendation['reason'], string> = {
  most_remaining: 'Most quota left.',
  resets_soon: 'Its weekly limit resets soon, so spend it first.',
  larger_plan: 'A bigger plan, so more room than the percentage suggests.'
}

function remainingText(recommendation: Recommendation): string {
  const percent = Math.round(recommendation.remainingPercent)
  return recommendation.projected ? `~${percent}% left after reset` : `${percent}% left`
}

/**
 * Answers the one question the app exists for: which account to use now.
 */
export const BestAccountBar = memo(function BestAccountBar({
  provider,
  accounts,
  activeAccountId,
  recommendation,
  switchingAccountId,
  onSwitch
}: BestAccountBarProps) {
  const { privacyMode, maskEmail } = usePrivacy()
  const label = (account: Account) =>
    (privacyMode ? maskEmail(account.email) : account.email) ?? 'Unnamed account'
  const best = recommendation
    ? accounts.find((account) => account.id === recommendation.accountId) ?? null
    : null

  // Re-evaluated once a minute so "back in 38m" keeps counting down.
  const nextBack = useTimeValue(useCallback(() => {
    if (best) return null
    const now = Date.now() / 1000
    let soonest: { id: string; at: number } | null = null
    for (const account of accounts) {
      if (!['healthy', 'refreshed'].includes(account.tokenHealth.status)) continue
      const at = accountBackAt(account, now)
      if (at !== null && (soonest === null || at < soonest.at)) soonest = { id: account.id, at }
    }
    return soonest ? `${soonest.id}|${soonest.at}` : null
  }, [accounts, best]), !best)

  if (best && recommendation) {
    const isActive = best.id === activeAccountId
    const plan = formatSubscriptionPlan(best.subscriptionPlan ?? best.quota?.planType, provider)
    const switching = switchingAccountId === best.id
    return (
      <section className={`best-account${isActive ? ' best-account-current' : ''}`} aria-label="Best account right now">
        <div className="best-account-copy">
          <span className="best-account-kicker">{isActive ? 'You are on the best account' : 'Best right now'}</span>
          <span className="best-account-line">
            <strong className={privacyMode ? 'privacy-masked' : 'allow-select'}>{label(best)}</strong>
            {plan && <span className="best-account-meta">{plan}</span>}
            <span className="best-account-meta">{remainingText(recommendation)}</span>
          </span>
          {!isActive && <span className="best-account-reason">{REASONS[recommendation.reason]}</span>}
        </div>
        {!isActive && (
          <button
            type="button"
            className="best-account-switch"
            onClick={() => onSwitch(best)}
            disabled={switching}
          >
            {switching && <Loader2 size={14} className="animate-spin" aria-hidden="true" />}
            Switch to it
          </button>
        )}
      </section>
    )
  }

  if (nextBack) {
    const [id, at] = nextBack.split('|')
    const account = accounts.find((candidate) => candidate.id === id)
    if (account) {
      return (
        <section className="best-account best-account-empty" aria-label="Best account right now">
          <div className="best-account-copy">
            <span className="best-account-kicker">Every account is used up</span>
            <span className="best-account-line">
              <strong className={privacyMode ? 'privacy-masked' : 'allow-select'}>{label(account)}</strong>
              <span className="best-account-meta">is back {formatTimeUntil(Number(at))}</span>
            </span>
          </div>
        </section>
      )
    }
  }

  return null
})
