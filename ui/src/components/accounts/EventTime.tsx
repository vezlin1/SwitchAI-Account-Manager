import { memo, useId, useMemo } from 'react'
import { useTimeValue } from '../../hooks/useSharedTicker'
import { formatEventTime, relativeEventTime } from '../../utils/eventTime'

export const EventTime = memo(function EventTime({ timestamp }: { timestamp: number | null | undefined }) {
  const relative = useTimeValue(() => relativeEventTime(timestamp), timestamp != null)
  const tooltipId = useId()
  const value = useMemo(() => formatEventTime(timestamp), [timestamp])
  if (!value) return <span className="text-ag-muted">Not reported</span>

  return (
    <span className="account-event-time" lang="en">
      <time dateTime={value.iso} tabIndex={0} aria-describedby={tooltipId}>
        {relative}
      </time>
      <span id={tooltipId} role="tooltip" className="account-event-time-tooltip">
        {value.exact}
      </span>
    </span>
  )
})
