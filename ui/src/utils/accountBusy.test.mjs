import test from 'node:test'
import assert from 'node:assert/strict'
import { computeAccountStatusFlags } from '../components/accounts/useAccountRowState.ts'

test('only the account with active work is marked refreshing', () => {
  const busy = new Set(['account:one:quota', 'refresh-all'])
  assert.equal(computeAccountStatusFlags('one', busy).isRefreshing, true)
  assert.equal(computeAccountStatusFlags('two', busy).isRefreshing, false)
  assert.equal(computeAccountStatusFlags('one', new Set()).isRefreshing, false)
})
