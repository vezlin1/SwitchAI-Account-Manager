import assert from 'node:assert/strict'
import test from 'node:test'
import {
  addBusyCount,
  busyCount,
  keyIsBusy
} from './busy.ts'

test('busy counters count concurrent operations per typed key', () => {
  let counters = {}

  counters = addBusyCount(counters, 'quota:account-a', 1)
  counters = addBusyCount(counters, 'quota:account-a', 1)
  counters = addBusyCount(counters, 'delete:account-b', 1)

  assert.equal(busyCount(counters, 'quota:account-a'), 2)
  assert.equal(busyCount(counters, 'delete:account-b'), 1)
  assert.equal(busyCount(counters, 'refresh-all'), 0)
  assert.equal(keyIsBusy(counters, 'quota:account-a'), true)
  assert.equal(keyIsBusy(counters, 'refresh-all'), false)

  counters = addBusyCount(counters, 'quota:account-a', -1)
  assert.equal(keyIsBusy(counters, 'quota:account-a'), true)
  counters = addBusyCount(counters, 'quota:account-a', -1)
  assert.equal(keyIsBusy(counters, 'quota:account-a'), false)
})

test('refresh-all participates in the same counted busy key set', () => {
  let counters = addBusyCount({}, 'refresh-all', 1)
  counters = addBusyCount(counters, 'refresh-all', 1)
  assert.equal(keyIsBusy(counters, 'refresh-all'), true)
  counters = addBusyCount(counters, 'refresh-all', -1)
  assert.equal(keyIsBusy(counters, 'refresh-all'), true)
  counters = addBusyCount(counters, 'refresh-all', -1)
  assert.equal(keyIsBusy(counters, 'refresh-all'), false)
})
