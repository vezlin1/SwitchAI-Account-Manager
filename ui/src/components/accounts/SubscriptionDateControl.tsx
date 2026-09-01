import { formatSubscriptionDate, formatSubscriptionPlan, isTimestampExpired, SUBSCRIPTION_LOCALE } from '../../utils/dateUtils'

type SubscriptionDateControlProps = {
  value: number | null
  plan: string | null
  provider?: 'codex' | 'gemini'
  hideNoPlanBadge?: boolean
}

function getPlanBadgeClass(planLabel: string | null): string {
  if (!planLabel) return 'plan-badge-default'
  const lower = planLabel.toLowerCase()
  if (lower.includes('ultra')) return 'plan-badge-ultra'
  if (lower.includes('enterprise') || lower.includes('business')) return 'plan-badge-enterprise'
  if (lower.includes('team')) return 'plan-badge-team'
  if (lower.includes('pro')) return 'plan-badge-pro'
  if (lower.includes('plus')) return 'plan-badge-plus'
  if (lower.includes('free')) return 'plan-badge-free'
  if (lower.includes('developer') || lower.includes('workspace')) return 'plan-badge-dev'
  if (lower.includes('advanced') || lower.includes('premium')) return 'plan-badge-premium'
  return 'plan-badge-default'
}

export function SubscriptionDateControl({
  value,
  plan,
  provider = 'codex',
  hideNoPlanBadge = false
}: SubscriptionDateControlProps) {
  const dateText = formatSubscriptionDate(value)
  const planLabel = formatSubscriptionPlan(plan, provider)
  const showDate = (provider !== 'gemini' || value != null) && dateText !== '—'
  const isExpired = isTimestampExpired(value)

  return (
    <div className="subscription-cell flex flex-col gap-1 items-start justify-center" lang={SUBSCRIPTION_LOCALE}>
      {planLabel ? (
        <span className={`plan-badge ${getPlanBadgeClass(planLabel)}`} title={planLabel}>
          {planLabel}
        </span>
      ) : hideNoPlanBadge ? null : (
        <span className="plan-badge plan-badge-default" title="No plan reported">
          No plan
        </span>
      )}
      {showDate && (
        <span
          className={`subscription-date-text text-[11px] font-normal tracking-normal ${
            isExpired ? 'text-amber-400/80 font-medium' : 'text-ag-muted'
          }`}
          title={dateText}
          dir="auto"
        >
          {dateText}
        </span>
      )}
    </div>
  )
}
