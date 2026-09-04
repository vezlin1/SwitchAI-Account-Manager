import assert from 'node:assert/strict'
import test from 'node:test'
import {
  accountRemainingPercent,
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
  assert.equal(recommendedAccount([low, best, relogin])?.id, 'best')
})

test('recommendation ignores hidden accounts when hiddenAccountIds is provided', () => {
  const visible = account('visible', 40) // 60% remaining
  const hidden = account('hidden', 10) // 90% remaining

  assert.equal(recommendedAccount([visible, hidden], ['hidden'])?.id, 'visible')
})
