// Collapse simultaneous requests; optionally run once more when data changed in flight.
export function createRefreshQueue(run: () => Promise<void>, replay = true) {
  let inFlight: Promise<void> | null = null
  let requested = false
  return () => {
    if (inFlight) {
      if (replay) requested = true
      return inFlight
    }
    requested = true
    inFlight = Promise.resolve().then(async () => {
      try {
        do {
          requested = false
          await run()
        } while (requested)
      } finally {
        inFlight = null
      }
    })
    return inFlight
  }
}
