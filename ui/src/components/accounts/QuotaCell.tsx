import { memo, useCallback, useMemo } from 'react'
import type { QuotaWindow } from '../../types'
import { formatTimeUntil, remainingPercent } from '../../utils/format'
import { formatEventTime } from '../../utils/eventTime'
import { quotaResetTime } from '../../utils/quotaWindows'
import { useTimeValue } from '../../hooks/useSharedTicker'

const EXHAUSTED_REMAINING_PERCENT = 0.5

function quotaClass(remaining: number): string {
  if (remaining <= 10) return 'quota-fill-danger'
  if (remaining <= 30) return 'quota-fill-warn'
  return 'quota-fill-good'
}

type QuotaCellProps = {
  window: QuotaWindow | null
  fetchedAt: number | null
  title: string
  /** Off in the table, where the column header already names the window. */
  showTitle?: boolean
  updateBlocked?: boolean
  isRefreshing?: boolean
}

export const QuotaCell = memo(function QuotaCell({
  window, fetchedAt, title, showTitle = true, updateBlocked = false, isRefreshing = false
}: QuotaCellProps) {
  const resetAt = quotaResetTime(window, fetchedAt)
  const reported = updateBlocked ? null : remainingPercent(window?.usedPercent)
  // Once the reset time has passed the window is known to be full again, even
  // before the next check confirms it.
  const resetPassed = useTimeValue(
    useCallback(() => resetAt != null && Date.now() / 1000 >= resetAt, [resetAt]),
    reported != null && resetAt != null
  )
  const projected = reported != null && resetPassed
  const remaining = projected ? 100 : reported
  const exhausted = remaining != null && !projected && remaining < EXHAUSTED_REMAINING_PERCENT
  const checkedAt = window?.fetchedAt ?? fetchedAt
  const checkedTime = useMemo(() => formatEventTime(checkedAt)?.exact, [checkedAt])
  const checkedTitle = remaining == null || !checkedTime
    ? undefined
    : projected
      ? `Limit reset after the last check (${checkedTime}); waiting for confirmation`
      : `Last checked: ${checkedTime}`
  const resetText = useTimeValue(useCallback(() => {
    if (resetAt == null) return exhausted ? 'Reset time unknown' : ''
    const now = Date.now()
    if (resetAt <= now / 1000) return 'Reset · not confirmed yet'
    return `${exhausted ? 'Back' : 'Resets'} ${formatTimeUntil(resetAt, now)}`
  }, [exhausted, resetAt]), remaining != null && resetAt != null)
  const roundedPercent = remaining == null ? undefined : Math.round(remaining)
  const valueText = remaining == null
    ? (updateBlocked ? '—' : 'No data yet')
    : exhausted
      ? 'Used up'
      : `${projected ? '~' : ''}${roundedPercent}% left`
  const valueTitle = remaining == null && updateBlocked ? 'Sign in again to see this limit' : checkedTitle

  return (
    <div className={`quota-cell min-w-[150px]${exhausted ? ' quota-cell-exhausted' : ''}`}>
      <div className={`quota-cell-head${showTitle ? '' : ' quota-cell-head-untitled'}`}>
        {showTitle && <span>{title}</span>}
        <span title={valueTitle} className={`quota-cell-value${remaining != null && !exhausted ? ' quota-cell-value-known' : ''}`}>
          {valueText}
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
              className={`quota-fill ${quotaClass(remaining)}${projected ? ' quota-fill-projected' : ''}`}
              style={{ transform: `scaleX(${remaining / 100})` }}
            />
          </div>
          {resetText && <div className="quota-cell-reset">{resetText}</div>}
        </>
      )}
    </div>
  )
})
