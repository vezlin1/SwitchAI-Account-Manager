type Clock = {
  now: () => number
  visible: () => boolean
  schedule: (callback: () => void, delay: number) => number
  cancel: (timer: number) => void
}

// One timer for all date labels. React compares each subscriber's selected value.
export function createTimeStore(clock: Clock) {
  const listeners = new Set<() => void>()
  let timer: number | undefined
  const stop = () => {
    if (timer !== undefined) clock.cancel(timer)
    timer = undefined
  }
  const notify = () => listeners.forEach((listener) => listener())
  const start = () => {
    if (timer !== undefined || !listeners.size || !clock.visible()) return
    timer = clock.schedule(() => {
      timer = undefined
      if (clock.visible()) notify()
      start()
    }, 60_000 - clock.now() % 60_000)
  }
  return {
    subscribe(listener: () => void) {
      listeners.add(listener)
      start()
      return () => {
        listeners.delete(listener)
        if (!listeners.size) stop()
      }
    },
    visibilityChanged() {
      stop()
      if (clock.visible()) {
        notify()
        start()
      }
    }
  }
}
