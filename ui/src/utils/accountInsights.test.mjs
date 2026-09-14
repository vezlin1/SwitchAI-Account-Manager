import assert from 'node:assert/strict'
import test from 'node:test'
import {
  accountRemainingPercent, quotaIsFresh,
  recommendedAccount
} from './accountInsights.ts'

function quotaWindow(usedPercent, limitWindowSeconds, resetAt) {
  return { usedPercent, limitWindowSeconds, resetAt, fetchedAt: 2_000 }
}

function account(id, usedPercent, status = 'healthy') {
  return {
    id,
    email: `${id}@example.com`,
    accountId: id,
    subscriptionExpiresAt: null,
    subscriptionPlan: null,
    subscriptionDetectedAt: null,
    tokensUpdatedAt: null,
    tokenHealth: { status, lastCheckedAt: null, lastRefreshedAt: null, lastError: null },
    quota: {
      planType: 'Plus',
      primary: quotaWindow(usedPercent, 18_000, 10_000),
      secondary: quotaWindow(null, null, null),
      fetchedAt: 2_000
    },
    createdAt: 1,
    lastLoginAt: 1,
    issues: { quota: null, subscription: null }
  }
}

test('recommendation chooses the healthy account with the largest tightest reserve', () => {
  const low = account('low', 80)
  const best = account('best', 20)
  const relogin = account('relogin', 1, 'needs_relogin')

  assert.equal(accountRemainingPercent(low), 20)
  assert.equal(recommendedAccount([low, best, relogin], [], 3000)?.id, 'best')
})

test('recommendation ignores hidden accounts when hiddenAccountIds is provided', () => {
  const visible = account('visible', 40) // 60% remaining
  const hidden = account('hidden', 10) // 90% remaining

  assert.equal(recommendedAccount([visible, hidden], ['hidden'], 3000)?.id, 'visible')
})

test('recommendations exclude stale quotas, failed connections, and invalid values', () => {
  const stale = account('stale', 0)
  stale.quota.fetchedAt = 1
  const failed = account('offline', 0, 'network_error')
  const invalid = account('invalid', NaN)
  const healthy = account('healthy', 40)
  const issue = account('issue', 0)
  issue.issues.quota = 'Request failed'
  assert.equal(recommendedAccount([stale, failed, invalid, issue, healthy], [], 4000)?.id, 'healthy')
  assert.equal(recommendedAccount([stale, failed, invalid, issue], [], 4000), null)
})

test('a crossed reset or one-hour-old quota needs a new check', () => {
  const value = account('test', 20)
  assert.equal(quotaIsFresh(value, 5599), true)
  assert.equal(quotaIsFresh(value, 5600), false)
  value.quota.primary.resetAt = 2500
  assert.equal(quotaIsFresh(value, 2500), false)
  assert.equal(quotaIsFresh(value, 1999), true)
  assert.equal(quotaIsFresh(value, 1900), false)
})
