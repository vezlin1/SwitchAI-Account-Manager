import assert from 'node:assert/strict'
import test from 'node:test'
import { subscribeFocusSync } from './focusSync.ts'

test('every rapid focus and restore signal requests catch-up', async () => {
  const visibility = new EventTarget()
  visibility.visibilityState = 'hidden'
  let listener
  let reloads = 0
  let stops = 0
  const dispose = subscribeFocusSync(async (fn) => { listener = fn; return () => stops++ }, visibility, async () => { reloads++ })
  listener({ payload: false })
  listener({ payload: true })
  listener({ payload: true })
  assert.equal(reloads, 2)
  visibility.dispatchEvent(new Event('visibilitychange'))
  assert.equal(reloads, 2)
  visibility.visibilityState = 'visible'
  visibility.dispatchEvent(new Event('visibilitychange'))
  assert.equal(reloads, 3)
  await Promise.resolve()
  dispose()
  listener({ payload: true })
  visibility.dispatchEvent(new Event('visibilitychange'))
  assert.equal(reloads, 3)
  assert.equal(stops, 1)
})

test('late listener registration is disposed after unmount', async () => {
  let complete
  let stops = 0
  const dispose = subscribeFocusSync(() => new Promise((resolve) => { complete = resolve }), new EventTarget(), async () => {})
  dispose()
  complete(() => stops++)
  await Promise.resolve()
  assert.equal(stops, 1)
})
