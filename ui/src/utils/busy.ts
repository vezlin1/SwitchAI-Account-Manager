export type BusyKey =
  | 'order'
  | 'settings:subscription-visibility'
  | 'refresh-all'
  | 'import:antigravity'
  | 'import:codex'
  | 'usage:clear'
  | 'usage:all'
  | `delete:${string}`
  | `switch:${string}`
  | `quota:${string}`
  | `subscription-detect:${string}`
  | `usage:${string}`

export type BusyCounters = Partial<Record<BusyKey, number>>

export function addBusyCount(
  counters: BusyCounters,
  key: BusyKey,
  delta: number
): BusyCounters {
  const nextCount = (counters[key] ?? 0) + delta
  const next = { ...counters }
  if (nextCount <= 0) delete next[key]
  else next[key] = nextCount
  return next
}

export function busyCount(counters: BusyCounters, key: BusyKey): number {
  return counters[key] ?? 0
}

export function keyIsBusy(counters: BusyCounters, key: BusyKey): boolean {
  return busyCount(counters, key) > 0
}
