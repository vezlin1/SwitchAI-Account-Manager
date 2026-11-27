import { memo, useCallback } from 'react'
import type { QuotaWindow } from '../../types'
import { formatTimeUntil, remainingPercent } from '../../utils/format'
import { quotaResetTime, quotaWindowIsStale } from '../../utils/quotaWindows'
import { useTimeValue } from '../../hooks/useSharedTicker'

function quotaClass(remaining: number): string {
  if (remaining <= 10) return 'quota-fill-danger'
  if (remaining <= 30) return 'quota-fill-warn'
  return 'quota-fill-good'
}

type QuotaCellProps = {
  window: QuotaWindow | null
  fetchedAt: number | null
  title: string
  updateBlocked?: boolean
  isRefreshing?: boolean
}

export const QuotaCell = memo(function QuotaCell({
  window, fetchedAt, title, updateBlocked = false, isRefreshing = false
}: QuotaCellProps) {
  const resetAt = quotaResetTime(window, fetchedAt)
  const remaining = remainingPercent(window?.usedPercent)
  const hasQuota = remaining != null
  const resetText = useTimeValue(useCallback(() => {
    if (!hasQuota || updateBlocked) return ''
    const now = Date.now()
    if (quotaWindowIsStale(window, fetchedAt, now / 1000)) return ''
    return resetAt ? `Reset ${formatTimeUntil(resetAt, now)}` : 'Reset time unavailable'
  }, [window, fetchedAt, hasQuota, updateBlocked, resetAt]), hasQuota && !updateBlocked)
  const available = remaining != null && resetText !== ''
  const roundedPercent = Math.round(remaining ?? 0)

  return (
    <div className="min-w-[160px]">
      <div className="flex items-center justify-between text-xs text-ag-muted mb-1">
        <span>{title}</span>
        <span className={`font-semibold tabular-nums ${available ? 'text-ag-text' : 'text-ag-muted'}`}>
          {available ? `${roundedPercent}% left` : 'Not available'}
        </span>
      </div>
      {available && (
        <>
          <div
            className={`quota-track${isRefreshing ? ' quota-track-refreshing' : ''}`}
            role="progressbar"
            aria-label={`${title} quota remaining`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={roundedPercent}
            aria-valuetext={`${roundedPercent}% remaining`}
          >
            <div
              className={`quota-fill ${quotaClass(remaining)}`}
              style={{ transform: `scaleX(${remaining / 100})` }}
            />
          </div>
          <div className="text-xs text-ag-muted mt-1">{resetText}</div>
        </>
      )}
    </div>
  )
})
