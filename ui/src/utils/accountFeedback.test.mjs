import assert from 'node:assert/strict'
import test from 'node:test'
import { accountAttention, quotaUpdateBlocked, splitRefreshWarnings } from './accountFeedback.ts'

test('only warnings without an account go to the provider banner', () => {
  const byScope = splitRefreshWarnings([
    { accountId: 'one', code: 'refresh_failed', message: 'Quota refresh failed for this account' },
    { accountId: 'two', code: 'refresh_failed', message: 'Account could not be read' },
    { accountId: null, message: 'ChatGPT refresh skipped' }
  ])
  assert.deepEqual([...byScope.byAccount], [
    ['one', { message: 'Quota refresh failed for this account', failed: true }],
    ['two', { message: 'Account could not be read', failed: true }]
  ])
  assert.deepEqual(byScope.general, ['ChatGPT refresh skipped'])

  const individual = splitRefreshWarnings([{ accountId: null, code: 'refresh_failed', message: 'Request timed out' }], 'one')
  assert.deepEqual(individual.byAccount.get('one'), { message: 'Request timed out', failed: true })
  assert.deepEqual(individual.general, [])
})

test('verification instructions take precedence over generic failures', () => {
  const account = {
    provider: 'gemini',
    issues: { quota: 'Quota refresh failed' },
    tokenHealth: { status: 'network_error', lastError: 'Verify your account to continue.' }
  }
  assert.deepEqual(accountAttention(account), {
    kind: 'verify',
    message: 'Verify your account to continue.'
  })
  assert.equal(accountAttention({ ...account, provider: 'codex' }).kind, 'quota')
})

test('a temporary subscription warning is separate from quota health', () => {
  const account = { tokenHealth: { status: 'healthy' }, issues: {} }
  assert.equal(accountAttention(account), null)
  assert.deepEqual(accountAttention(account, { area: 'subscription', message: 'Plan request timed out', failed: true }), {
    kind: 'subscription',
    message: 'Plan request timed out'
  })
  assert.equal(accountAttention({ ...account, tokenHealth: { status: 'needs_relogin' } }).kind, 'relogin')
})

test('an account warning after a successful refresh keeps the current quota', () => {
  const account = { tokenHealth: { status: 'healthy' }, issues: {} }
  const attention = accountAttention(account, { area: 'quota', message: 'Token update skipped', failed: false })
  assert.equal(attention.kind, 'notice')
  assert.equal(quotaUpdateBlocked(attention), false)
})
