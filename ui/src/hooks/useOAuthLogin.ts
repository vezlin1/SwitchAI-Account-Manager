import { useCallback, useEffect, useRef, useState } from 'react'
import { api, describeIpcError } from '../api'
import type { Account, AccountProvider } from '../types'
import { runOAuthPoll, type OAuthPollResult } from '../utils/oauthPoll'

type UseOAuthLoginArgs = {
  onCompleted: () => Promise<void>
}

type ActivePoll = {
  flowId: string
  startedAt: number
  cancelled: boolean
  provider: AccountProvider
}

export function useOAuthLogin({ onCompleted }: UseOAuthLoginArgs) {
  const [busy, setBusy] = useState(false)
  const [errors, setErrors] = useState<Record<AccountProvider, string | null>>({
    codex: null,
    gemini: null
  })
  const activePollRef = useRef<ActivePoll | null>(null)
  const startingRef = useRef(false)
  const startAttemptRef = useRef(0)
  const onCompletedRef = useRef(onCompleted)

  useEffect(() => {
    onCompletedRef.current = onCompleted
  }, [onCompleted])

  const stopPolling = useCallback(() => {
    const active = activePollRef.current
    if (active) {
      active.cancelled = true
      activePollRef.current = null
    }
  }, [])

  const setProviderError = useCallback((provider: AccountProvider, msg: string | null) => {
    setErrors((prev) => ({ ...prev, [provider]: msg }))
  }, [])

  const clearError = useCallback((provider?: AccountProvider) => {
    if (provider) {
      setErrors((prev) => ({ ...prev, [provider]: null }))
    } else {
      setErrors({ codex: null, gemini: null })
    }
  }, [])

  const getError = useCallback((provider: AccountProvider) => {
    return errors[provider]
  }, [errors])

  const cancelLogin = useCallback(() => {
    const active = activePollRef.current
    startAttemptRef.current += 1
    startingRef.current = false
    stopPolling()
    setBusy(false)
    if (active) {
      setProviderError(active.provider, null)
      void api.cancelOAuthFlow(active.flowId).catch(() => undefined)
    }
  }, [stopPolling, setProviderError])

  useEffect(() => cancelLogin, [cancelLogin])

  const startLogin = useCallback(async (targetAccount?: Account, provider?: string) => {
    if (startingRef.current || activePollRef.current) return
    const attempt = startAttemptRef.current + 1
    startAttemptRef.current = attempt
    startingRef.current = true
    const currentProv: AccountProvider = (targetAccount?.provider ?? provider ?? 'codex') === 'gemini' ? 'gemini' : 'codex'

    try {
      setBusy(true)
      setProviderError(currentProv, null)
      stopPolling()

      const targetProvider = targetAccount?.provider ?? provider ?? null
      const flow = await api.startOAuthFlow(targetAccount?.id ?? null, targetProvider)
      startingRef.current = false
      if (startAttemptRef.current !== attempt) {
        void api.cancelOAuthFlow(flow.flowId).catch(() => undefined)
        return
      }
      const startedAt = Date.now()
      const active: ActivePoll = {
        flowId: flow.flowId,
        startedAt,
        cancelled: false,
        provider: currentProv
      }
      const poll: Promise<OAuthPollResult> = runOAuthPoll({
          flowId: flow.flowId,
          startedAt,
          isCancelled: () => active.cancelled,
          poll: (flowId) => api.getOAuthStatus(flowId)
        })
      activePollRef.current = active

      try {
        await api.openExternalUrl(flow.authorizationUrl)
      } catch (err) {
        active.cancelled = true
        activePollRef.current = null
        void api.cancelOAuthFlow(active.flowId).catch(() => undefined)
        setBusy(false)
        setProviderError(currentProv, describeIpcError(err))
        return
      }

      const result = await poll
      if (activePollRef.current !== active) return
      activePollRef.current = null
      setBusy(false)

      if (result.terminal === 'completed') {
        await onCompletedRef.current()
      } else if (result.terminal === 'cancelled_by_backend') {
        setProviderError(currentProv, 'OAuth login was cancelled')
      } else if (result.terminal === 'error') {
        setProviderError(currentProv, result.message)
      } else {
        setProviderError(currentProv, 'OAuth login timed out after 10 minutes. Start again when ready.')
        void api.cancelOAuthFlow(active.flowId).catch(() => undefined)
      }
    } catch (err) {
      if (startAttemptRef.current !== attempt) return
      startingRef.current = false
      stopPolling()
      setBusy(false)
      setProviderError(currentProv, describeIpcError(err))
    }
  }, [stopPolling, setProviderError])

  return {
    busy,
    errors,
    error: errors.codex ?? errors.gemini,
    getError,
    clearError,
    startLogin,
    cancelLogin
  }
}
