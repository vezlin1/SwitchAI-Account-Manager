const exactFormatter = new Intl.DateTimeFormat('en-US', {
  year: 'numeric', month: 'short', day: 'numeric',
  hour: 'numeric', minute: '2-digit', second: '2-digit', timeZoneName: 'short'
})
const relativeFormatter = new Intl.RelativeTimeFormat('en', { numeric: 'always' })

export function relativeEventTime(timestamp: number | null | undefined, now = Date.now()) {
  if (timestamp == null || !Number.isFinite(timestamp)) return null
  const date = new Date(timestamp * 1000)
  if (!Number.isFinite(date.getTime())) return null
  const seconds = Math.round((date.getTime() - now) / 1000)
  const absolute = Math.abs(seconds)
  const units = [
    ['year', 365 * 86400], ['month', 30 * 86400], ['day', 86400],
    ['hour', 3600], ['minute', 60]
  ] as const
  const unit = units.find(([, size]) => absolute >= size)
  const relative = unit
    ? relativeFormatter.format(Math.trunc(seconds / unit[1]), unit[0])
    : 'Just now'
  return relative
}

export function formatEventTime(timestamp: number | null | undefined, now = Date.now()) {
  const relative = relativeEventTime(timestamp, now)
  if (relative === null || timestamp == null) return null
  const date = new Date(timestamp * 1000)
  return { relative, exact: exactFormatter.format(date), iso: date.toISOString() }
}
