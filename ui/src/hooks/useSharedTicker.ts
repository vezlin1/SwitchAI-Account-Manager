import { useSyncExternalStore } from 'react'

const listeners = new Set<() => void>()
let intervalId: ReturnType<typeof setInterval> | null = null

function notifyListeners() {
  if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
    return
  }
  listeners.forEach((listener) => listener())
}

function startTimer() {
  if (!intervalId && typeof window !== 'undefined') {
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
      return
    }
    intervalId = setInterval(notifyListeners, 10000)
  }
}

function stopTimer() {
  if (intervalId && listeners.size === 0) {
    clearInterval(intervalId)
    intervalId = null
  }
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
      if (listeners.size > 0) {
        startTimer()
        notifyListeners()
      }
    } else if (intervalId) {
      clearInterval(intervalId)
      intervalId = null
    }
  })
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  startTimer()
  return () => {
    listeners.delete(listener)
    stopTimer()
  }
}

const subscribeNoop = () => () => undefined

export function useSharedTicker(getSnapshot: () => string, enabled: boolean): string {
  return useSyncExternalStore(enabled ? subscribe : subscribeNoop, getSnapshot, getSnapshot)
}
