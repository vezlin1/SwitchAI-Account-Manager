export function remainingPercent(value: number | null | undefined): number | null {
  if (value == null || !Number.isFinite(value) || value < 0 || value > 100) return null
  return 100 - value
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

/** "5 min ago", "2 h ago" for status lines that refresh once a minute. */
export function formatAgo(unixTsSeconds: number, now = Date.now()): string {
  const minutes = Math.max(0, Math.floor((now - unixTsSeconds * 1000) / 60000))
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} h ago`
  return `${Math.floor(hours / 24)} d ago`
}
