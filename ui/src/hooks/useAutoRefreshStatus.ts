import { useCallback, useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api } from '../api'
import type { AutoRefreshStatus } from '../types'
import { sameValue } from '../utils/stateMerge'

function mergeStatus(previous: AutoRefreshStatus | null, next: AutoRefreshStatus) {
  if (sameValue(previous, next)) return previous
  return { ...next, refreshingAccountIds: sameValue(previous?.refreshingAccountIds, next.refreshingAccountIds)
    ? previous!.refreshingAccountIds : next.refreshingAccountIds }
}

export function useAutoRefreshStatus() {
  const [status, setStatus] = useState<AutoRefreshStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const refreshInFlightRef = useRef<Promise<AutoRefreshStatus> | null>(null)

  const clearError = useCallback(() => setError(null), [])

  const refreshStatus = useCallback(() => {
    if (refreshInFlightRef.current) {
      return refreshInFlightRef.current
    }

    const request = (async () => {
      try {
        const next = await api.getAutoRefreshStatus()
        setStatus((previous) => mergeStatus(previous, next))
        setError(null)

        return next
      } catch (err) {
        setError(String(err))
        throw err
      } finally {
        refreshInFlightRef.current = null
      }
    })()

    refreshInFlightRef.current = request
    return request
  }, [])

  useEffect(() => {
    let disposed = false
    let unlisten: UnlistenFn | undefined
    let fallbackTimer: number | undefined
    const load = async () => {
      try {
        await refreshStatus()
      } catch {
        // refreshStatus stores the error for the visible status banner.
      }
    }

    void load()
    void listen<AutoRefreshStatus>('auto-refresh-status-changed', ({ payload }) => {
      setStatus((previous) => mergeStatus(previous, payload))
      setError(null)
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch(() => {
      if (!disposed) {
        fallbackTimer = window.setInterval(() => {
          void load()
        }, 30000)
      }
    })

    return () => {
      disposed = true
      unlisten?.()
      if (fallbackTimer !== undefined) window.clearInterval(fallbackTimer)
    }
  }, [refreshStatus])

  return {
    status,
    error,
    clearError,
    refreshStatus
  }
}
