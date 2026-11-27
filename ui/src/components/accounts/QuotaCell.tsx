import { memo, useCallback, useMemo } from 'react'
import type { QuotaWindow } from '../../types'
import { formatTimeUntil, remainingPercent } from '../../utils/format'
import { formatEventTime } from '../../utils/eventTime'
import { quotaResetTime } from '../../utils/quotaWindows'
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
  const remaining = updateBlocked ? null : remainingPercent(window?.usedPercent)
  const checkedAt = window?.fetchedAt ?? fetchedAt
  const checkedTime = useMemo(() => formatEventTime(checkedAt)?.exact, [checkedAt])
  const checkedTitle = remaining != null && checkedTime ? `Last checked: ${checkedTime}` : undefined
  const resetText = useTimeValue(useCallback(() => {
    if (resetAt == null) return 'Reset time unavailable'
    const now = Date.now()
    return resetAt > now / 1000 ? `Reset ${formatTimeUntil(resetAt, now)}` : ''
  }, [resetAt]), remaining != null && resetAt != null)
  const roundedPercent = remaining == null ? undefined : Math.round(remaining)

  return (
    <div className="min-w-[160px]">
      <div className="flex items-center justify-between text-xs text-ag-muted mb-1">
        <span>{title}</span>
        <span title={checkedTitle} className={`font-semibold tabular-nums ${remaining != null ? 'text-ag-text' : 'text-ag-muted'}`}>
          {remaining != null ? `${roundedPercent}% left` : 'Not available'}
        </span>
      </div>
      {remaining != null && (
        <>
          <div
            className={`quota-track${isRefreshing ? ' quota-track-refreshing' : ''}`}
            role="progressbar"
            aria-label={`${title} quota remaining`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={roundedPercent}
            aria-valuetext={checkedTitle ? `${roundedPercent}% remaining. ${checkedTitle}` : `${roundedPercent}% remaining`}
          >
            <div
              className={`quota-fill ${quotaClass(remaining)}`}
              style={{ transform: `scaleX(${remaining / 100})` }}
            />
          </div>
          {resetText && <div className="text-xs text-ag-muted mt-1">{resetText}</div>}
        </>
      )}
    </div>
  )
})
