import { useSyncExternalStore } from 'react'
import { createTimeStore } from '../utils/timeStore'

const clock = createTimeStore({
  now: Date.now,
  visible: () => typeof document !== 'undefined' && document.visibilityState === 'visible',
  schedule: (callback, delay) => window.setTimeout(callback, delay),
  cancel: (timer) => window.clearTimeout(timer)
})
if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', clock.visibilityChanged)
}
const noSubscription = () => () => {}

export function useTimeValue<T extends string | number | boolean | null | undefined>(
  select: () => T, enabled = true
): T {
  return useSyncExternalStore(enabled ? clock.subscribe : noSubscription, select, select)
}

export function useSharedTicker(enabled = true): number {
  return useTimeValue(() => Math.floor(Date.now() / 60_000), enabled)
}
