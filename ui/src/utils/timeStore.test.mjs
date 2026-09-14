import test from 'node:test'
import assert from 'node:assert/strict'
import { createTimeStore } from './timeStore.ts'

test('all subscribers share one minute timer, pause while hidden, and catch up on restore', () => {
  let now = 12500
  let visible = true
  let id = 0
  const timers = new Map()
  const store = createTimeStore({
    now: () => now, visible: () => visible,
    schedule: (callback, delay) => { timers.set(++id, { callback, delay }); return id },
    cancel: (key) => timers.delete(key)
  })
  let first = 0, second = 0
  const unsubscribe = store.subscribe(() => first++)
  const unsubscribeSecond = store.subscribe(() => second++)
  assert.equal(timers.size, 1)
  const [key, timer] = [...timers][0]
  assert.equal(timer.delay, 47500)
  timers.delete(key)
  now = 60000
  timer.callback()
  assert.equal(first, 1)
  assert.equal(second, 1)
  assert.equal(timers.size, 1)
  assert.equal([...timers.values()][0].delay, 60000)
  visible = false
  store.visibilityChanged()
  assert.equal(timers.size, 0)
  now = 300000
  visible = true
  store.visibilityChanged()
  assert.equal(first, 2)
  assert.equal(second, 2)
  assert.equal(timers.size, 1)
  unsubscribe()
  assert.equal(timers.size, 1)
  unsubscribeSecond()
  assert.equal(timers.size, 0)
})
