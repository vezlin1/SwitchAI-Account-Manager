export function clampPercent(value: number | null | undefined): number {
  if (value == null || Number.isNaN(value)) return 0
  if (value < 0) return 0
  if (value > 100) return 100
  return value
}

export function remainingPercent(value: number | null | undefined): number | null {
  if (value == null || Number.isNaN(value)) return null
  return clampPercent(100 - clampPercent(value))
}

export function formatRemainingPercent(value: number | null | undefined): string {
  const remaining = remainingPercent(value)
  if (remaining == null) return '-'
  return `${Math.round(remaining)}%`
}

export function formatTimeUntil(unixTsSeconds: number | null | undefined): string {
  if (!unixTsSeconds) return '-'

  const now = Date.now()
  const target = unixTsSeconds * 1000
  const deltaMs = target - now
  const absMs = Math.abs(deltaMs)

  const totalMinutes = Math.floor(absMs / 60000)
  const days = Math.floor(totalMinutes / (60 * 24))
  const hours = Math.floor((totalMinutes % (60 * 24)) / 60)
  const minutes = totalMinutes % 60

  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0) parts.push(`${hours}h`)
  if (minutes > 0 || parts.length === 0) parts.push(`${minutes}m`)

  const body = parts.slice(0, 2).join(' ')
  if (deltaMs >= 0) {
    return `in ${body}`
  }
  if (totalMinutes === 0) {
    return 'just now'
  }
  return `due (${body} ago)`
}

export function isTimePast(unixTsSeconds: number | null | undefined): boolean {
  if (!unixTsSeconds) return false
  return unixTsSeconds * 1000 <= Date.now()
}
