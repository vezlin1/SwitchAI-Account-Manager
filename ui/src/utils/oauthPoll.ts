export const OAUTH_POLL_INTERVAL_MS = 1500
export const OAUTH_POLL_TIMEOUT_MS = 10 * 60 * 1000

export type OAuthPollResult =
  | { terminal: 'completed' }
  | { terminal: 'error'; message: string }
  | { terminal: 'cancelled_by_backend' }
  | { terminal: 'timeout' }
  | { terminal: 'cancelled' }

type OAuthPollOptions = {
  poll: (flowId: string) => Promise<{
    status: string
    error?: string | null
  }>
  flowId: string
  startedAt: number
  timeoutMs?: number
  intervalMs?: number
  isCancelled?: () => boolean
  sleep?: (ms: number) => Promise<void>
}

export async function runOAuthPoll({
  poll,
  flowId,
  startedAt,
  timeoutMs = OAUTH_POLL_TIMEOUT_MS,
  intervalMs = OAUTH_POLL_INTERVAL_MS,
  isCancelled,
  sleep
}: OAuthPollOptions): Promise<OAuthPollResult> {
  const wait = sleep ?? ((ms: number) => new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  }))
  let lastPollError: string | null = null

  // Chained, single-flight: one status attempt completes before the next starts.
  while (true) {
    if (isCancelled?.()) return { terminal: 'cancelled' }
    const remainingMs = timeoutMs - (Date.now() - startedAt)
    if (remainingMs <= 0) return { terminal: 'timeout' }

    let timeoutHandle: ReturnType<typeof setTimeout> | undefined
    try {
      const status = await Promise.race([
        poll(flowId),
        new Promise<'__timeout'>((resolve) => {
          timeoutHandle = setTimeout(() => resolve('__timeout'), remainingMs)
        })
      ])
      if (isCancelled?.()) return { terminal: 'cancelled' }
      if (status === '__timeout') return { terminal: 'timeout' }
      if (status.status === 'completed') return { terminal: 'completed' }
      if (status.status === 'cancelled') {
        return { terminal: 'cancelled_by_backend' }
      }
      if (status.status === 'error') {
        return {
          terminal: 'error',
          message: status.error ?? lastPollError ?? 'OAuth login failed'
        }
      }
      lastPollError = null
    } catch (err) {
      lastPollError = String(err)
    } finally {
      if (timeoutHandle !== undefined) {
        clearTimeout(timeoutHandle)
      }
    }

    await wait(intervalMs)
  }
}
