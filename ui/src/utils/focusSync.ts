// Focus and restoration are catch-up signals; reload owns request deduplication.
export function subscribeFocusSync(
  subscribe: (listener: (event: { payload: boolean }) => void) => Promise<() => void>,
  visibility: Pick<Document, 'visibilityState' | 'addEventListener' | 'removeEventListener'>,
  reload: () => Promise<unknown>
): () => void {
  let disposed = false
  let unlisten: (() => void) | undefined
  const catchUp = () => { if (!disposed) void reload() }
  const onVisible = () => { if (visibility.visibilityState === 'visible') catchUp() }
  visibility.addEventListener('visibilitychange', onVisible)
  void subscribe(({ payload }) => { if (payload) catchUp() }).then((stop) => {
    if (disposed) stop()
    else unlisten = stop
  }).catch(() => undefined)
  return () => {
    disposed = true
    unlisten?.()
    visibility.removeEventListener('visibilitychange', onVisible)
  }
}
