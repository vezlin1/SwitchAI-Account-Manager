import { useCallback, useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api } from '../api'
import type { AutoRefreshStatus } from '../types'

type VisibleStatus = Pick<AutoRefreshStatus, 'inFlight' | 'lastFinishedAt'>

export function useAutoRefreshStatus(initialStatus: VisibleStatus | null) {
  const [status, setStatus] = useState<VisibleStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const refreshInFlightRef = useRef<Promise<AutoRefreshStatus> | null>(null)

  const clearError = useCallback(() => setError(null), [])
  const publishStatus = useCallback(({ inFlight, lastFinishedAt }: VisibleStatus) => {
    setStatus((current) => (
      current?.inFlight === inFlight && current.lastFinishedAt === lastFinishedAt
        ? current
        : { inFlight, lastFinishedAt }
    ))
    setError(null)
  }, [])

  const refreshStatus = useCallback(() => {
    if (refreshInFlightRef.current) {
      return refreshInFlightRef.current
    }

    const request = (async () => {
      try {
        const next = await api.getAutoRefreshStatus()
        publishStatus(next)

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
  }, [publishStatus])

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

    void listen<AutoRefreshStatus>('auto-refresh-status-changed', ({ payload }) => {
      publishStatus(payload)
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch(() => {
      if (!disposed) {
        void load()
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
  }, [publishStatus, refreshStatus])

  return {
    status: status ?? initialStatus,
    error,
    clearError
  }
}
