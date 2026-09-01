import assert from 'node:assert/strict'
import test from 'node:test'
import { runOAuthPoll } from './oauthPoll.ts'

test('polling completes through the terminal status', async () => {
  let calls = 0
  const result = await runOAuthPoll({
    flowId: 'flow-1',
    startedAt: Date.now(),
    timeoutMs: 5000,
    intervalMs: 1,
    poll: async () => {
      calls += 1
      return { status: calls === 1 ? 'waiting_callback' : 'completed' }
    },
    sleep: () => Promise.resolve()
  })

  assert.deepEqual(result, { terminal: 'completed' })
  assert.equal(calls, 2)
})

test('polling surfaces the backend error message', async () => {
  const result = await runOAuthPoll({
    flowId: 'flow-1',
    startedAt: Date.now(),
    timeoutMs: 5000,
    intervalMs: 1,
    poll: async () => ({
      status: 'error',
      error: 'callback rejected'
    }),
    sleep: () => Promise.resolve()
  })

  assert.deepEqual(result, { terminal: 'error', message: 'callback rejected' })
})

test('polling treats a backend cancelled status as terminal without completion', async () => {
  const result = await runOAuthPoll({
    flowId: 'flow-1',
    startedAt: Date.now(),
    timeoutMs: 5000,
    intervalMs: 1,
    poll: async () => ({ status: 'cancelled' }),
    sleep: () => Promise.resolve()
  })

  assert.deepEqual(result, { terminal: 'cancelled_by_backend' })
})

test('polling times out after ten minutes and never overlaps attempts', async () => {
  let inFlight = 0
  let maxInFlight = 0
  const result = await runOAuthPoll({
    flowId: 'flow-1',
    startedAt: Date.now(),
    timeoutMs: 2,
    intervalMs: 1,
    poll: async () => {
      inFlight += 1
      maxInFlight = Math.max(maxInFlight, inFlight)
      await new Promise((resolve) => setTimeout(resolve, 1))
      inFlight -= 1
      return { status: 'waiting_callback' }
    },
    sleep: () => Promise.resolve()
  })

  assert.equal(result.terminal, 'timeout')
  assert.equal(maxInFlight, 1)
})

test('cancellation stops the chained poll', async () => {
  let calls = 0
  const result = await runOAuthPoll({
    flowId: 'flow-1',
    startedAt: Date.now(),
    timeoutMs: 5000,
    intervalMs: 1,
    isCancelled: () => calls >= 1,
    poll: async () => {
      calls += 1
      return { status: 'waiting_callback' }
    },
    sleep: () => Promise.resolve()
  })

  assert.deepEqual(result, { terminal: 'cancelled' })
  assert.equal(calls, 1)
})
