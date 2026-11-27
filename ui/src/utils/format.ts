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

export function formatTimeUntil(unixTsSeconds: number, now = Date.now()): string {
  const totalMinutes = Math.floor(Math.max(0, unixTsSeconds * 1000 - now) / 60000)
  const days = Math.floor(totalMinutes / (60 * 24))
  const hours = Math.floor((totalMinutes % (60 * 24)) / 60)
  const minutes = totalMinutes % 60

  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0) parts.push(`${hours}h`)
  if (minutes > 0 || parts.length === 0) parts.push(`${minutes}m`)

  return `in ${parts.slice(0, 2).join(' ')}`
}
