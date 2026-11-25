import test from 'node:test'
import assert from 'node:assert/strict'
import { createRefreshQueue } from './refreshQueue.ts'

test('state changes arriving during a request produce one follow-up without overlap', async () => {
  let calls = 0
  let release
  const queue = createRefreshQueue(async () => {
    calls++
    if (calls === 1) await new Promise((resolve) => { release = resolve })
  })
  const first = queue()
  await Promise.resolve()
  assert.equal(queue(), first)
  assert.equal(queue(), first)
  assert.equal(calls, 1)
  release()
  await first
  assert.equal(calls, 2)
})

test('process polling shares an in-flight request and recovers after a failure', async () => {
  let calls = 0
  let release
  const queue = createRefreshQueue(async () => {
    calls++
    if (calls === 1) {
      await new Promise((resolve) => { release = resolve })
      throw new Error('offline')
    }
  }, false)
  const first = queue()
  await Promise.resolve()
  assert.equal(queue(), first)
  release()
  await assert.rejects(first, /offline/)
  assert.equal(calls, 1)
  await queue()
  assert.equal(calls, 2)
})
