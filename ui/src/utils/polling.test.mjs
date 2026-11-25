import assert from 'node:assert/strict'
import test from 'node:test'
import { createCompletionPoller } from './polling.ts'

function fakeTimers() {
  let nextId = 0
  const pending = new Map()
  return {
    pending,
    schedule(callback, delayMs) {
      const id = ++nextId
      pending.set(id, { callback, delayMs })
      return id
    },
    cancel(id) { pending.delete(id) },
    fire() {
      const [id, task] = pending.entries().next().value
      pending.delete(id)
      task.callback()
    }
  }
}

function deferred() {
  let resolve
  const promise = new Promise((done) => { resolve = done })
  return { promise, resolve }
}

test('slow polling and concurrent focus refreshes share one request and wait for completion', async () => {
  const timers = fakeTimers()
  const first = deferred()
  let calls = 0
  const poller = createCompletionPoller({
    run: () => { calls += 1; return calls === 1 ? first.promise : Promise.resolve() },
    intervalMs: 5000,
    schedule: timers.schedule,
    cancel: timers.cancel
  })
  poller.setActive(true)
  const request = poller.request()
  assert.equal(poller.request(), request)
  await Promise.resolve()
  assert.equal(calls, 1)
  assert.equal(timers.pending.size, 0)
  first.resolve()
  await request
  assert.equal(timers.pending.size, 1)
  assert.equal([...timers.pending.values()][0].delayMs, 5000)
  timers.fire()
  await poller.request()
  assert.equal(calls, 2)
  assert.equal(timers.pending.size, 1)
  poller.dispose()
})

test('hidden polling stops timers and resumes with one refresh; disposal stops late completions', async () => {
  const timers = fakeTimers()
  const last = deferred()
  let calls = 0
  const poller = createCompletionPoller({
    run: () => { calls += 1; return calls === 3 ? last.promise : Promise.resolve() },
    intervalMs: 5000,
    schedule: timers.schedule,
    cancel: timers.cancel
  })
  await poller.request()
  assert.equal(calls, 0)
  poller.setActive(true)
  await poller.request()
  assert.equal(timers.pending.size, 1)
  poller.setActive(false)
  await poller.request()
  assert.equal(calls, 1)
  assert.equal(timers.pending.size, 0)
  poller.setActive(true)
  poller.setActive(true)
  await poller.request()
  assert.equal(calls, 2)
  const request = poller.request()
  await Promise.resolve()
  assert.equal(calls, 3)
  poller.dispose()
  last.resolve()
  await request
  await poller.request()
  assert.equal(calls, 3)
  assert.equal(timers.pending.size, 0)
})

test('a failed request permits a later polling retry', async () => {
  const timers = fakeTimers()
  let calls = 0
  const poller = createCompletionPoller({
    run: () => { calls += 1; return Promise.reject(new Error('IPC unavailable')) },
    intervalMs: 5000,
    schedule: timers.schedule,
    cancel: timers.cancel
  })
  poller.setActive(true)
  await poller.request()
  timers.fire()
  await poller.request()
  assert.equal(calls, 2)
  poller.dispose()
  assert.equal(timers.pending.size, 0)
})
