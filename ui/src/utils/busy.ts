import type { AccountProvider } from '../types'

export type BusyKey =
  | 'order'
  | 'settings:subscription-visibility'
  | 'refresh-all'
  | `import:${AccountProvider}`
  | `delete:${string}`
  | `switch:${string}`
  | `quota:${string}`
  | `subscription-detect:${string}`

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
