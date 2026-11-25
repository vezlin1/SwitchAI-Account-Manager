import assert from 'node:assert/strict'
import test from 'node:test'
import { addBusyCount } from './busy.ts'

test('busy counters count concurrent operations per typed key', () => {
  let counters = {}

  counters = addBusyCount(counters, 'quota:account-a', 1)
  counters = addBusyCount(counters, 'quota:account-a', 1)
  counters = addBusyCount(counters, 'delete:account-b', 1)

  assert.deepEqual(counters, { 'quota:account-a': 2, 'delete:account-b': 1 })

  counters = addBusyCount(counters, 'quota:account-a', -1)
  assert.equal(counters['quota:account-a'], 1)
  counters = addBusyCount(counters, 'quota:account-a', -1)
  assert.deepEqual(counters, { 'delete:account-b': 1 })
})

test('refresh-all participates in the same counted busy key set', () => {
  let counters = addBusyCount({}, 'refresh-all', 1)
  counters = addBusyCount(counters, 'refresh-all', 1)
  assert.equal(counters['refresh-all'], 2)
  counters = addBusyCount(counters, 'refresh-all', -1)
  assert.equal(counters['refresh-all'], 1)
  counters = addBusyCount(counters, 'refresh-all', -1)
  assert.deepEqual(counters, {})
})
