type Timer = ReturnType<typeof setTimeout>

type PollingOptions = {
  run: () => Promise<unknown>
  intervalMs: number
  schedule?: (callback: () => void, delayMs: number) => Timer
  cancel?: (timer: Timer) => void
}

// A slow IPC request must finish before its next polling interval starts.
// Focus and visibility refreshes join that request instead of overlapping it.
export function createCompletionPoller({
  run,
  intervalMs,
  schedule = setTimeout,
  cancel = clearTimeout
}: PollingOptions) {
  let active = false
  let disposed = false
  let timer: Timer | null = null
  let inFlight: Promise<void> | null = null

  const clearTimer = () => {
    if (timer !== null) cancel(timer)
    timer = null
  }

  const request = (): Promise<void> => {
    if (disposed || !active) return Promise.resolve()
    if (inFlight) return inFlight
    clearTimer()
    // Resolve run on a microtask so inFlight is assigned even if it throws.
    inFlight = Promise.resolve().then(run).then(() => undefined, () => undefined).finally(() => {
      inFlight = null
      if (disposed || !active) return
      timer = schedule(() => {
        timer = null
        void request()
      }, intervalMs)
    })
    return inFlight
  }

  return {
    request,
    setActive(next: boolean) {
      if (disposed || active === next) return
      active = next
      if (active) void request()
      else clearTimer()
    },
    dispose() {
      disposed = true
      active = false
      clearTimer()
    }
  }
}
