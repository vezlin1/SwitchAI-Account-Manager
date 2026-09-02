import { memo, useEffect, useState } from 'react'
import { formatRemainingPercent, formatTimeUntil, isTimePast, remainingPercent } from '../../utils/format'

function quotaClass(remaining: number, hasQuota: boolean): string {
  if (!hasQuota) return ''
  if (remaining <= 10) return 'quota-fill-danger'
  if (remaining <= 30) return 'quota-fill-warn'
  return 'quota-fill-good'
}

type QuotaCellProps = {
  value: number | null | undefined
  resetAt: number | null | undefined
  title: string
  isRefreshing?: boolean
}

export const QuotaCell = memo(function QuotaCell({ value, resetAt, title, isRefreshing = false }: QuotaCellProps) {
  const [, setTick] = useState(0)
  const remaining = remainingPercent(value)
  const barPercent = remaining ?? 0
  const hasQuota = remaining != null

  useEffect(() => {
    if (!resetAt) return
    const interval = setInterval(() => {
      setTick((t) => (t + 1) % 10000)
    }, 30000)
    return () => clearInterval(interval)
  }, [resetAt])

  return (
    <div className="min-w-[160px]">
      <div className="flex items-center justify-between text-xs text-ag-muted mb-1">
        <span>{title}</span>
        <span className="font-semibold text-ag-text tabular-nums transition-opacity duration-300">
          {hasQuota ? `${formatRemainingPercent(value)} left` : 'Not available'}
        </span>
      </div>
      <div
        className={`quota-track${isRefreshing ? ' quota-track-refreshing' : ''}`}
        role="progressbar"
        aria-label={`${title} quota remaining`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={hasQuota ? Math.round(barPercent) : undefined}
        aria-valuetext={hasQuota ? `${Math.round(barPercent)}% remaining` : 'Not available'}
      >
        <div
          className={`quota-fill ${quotaClass(barPercent, hasQuota)}`}
          style={{ transform: `scaleX(${barPercent / 100})` }}
        />
      </div>
      <div className="text-xs text-ag-muted mt-1 flex items-center gap-1.5">
        {resetAt ? (
          isTimePast(resetAt) ? (
            <span className="text-blue-400 font-medium animate-pulse">
              reset {formatTimeUntil(resetAt)}
            </span>
          ) : (
            <span>reset {formatTimeUntil(resetAt)}</span>
          )
        ) : (
          'reset not reported'
        )}
      </div>
    </div>
  )
})
