import assert from 'node:assert/strict'
import test from 'node:test'
import {
  isStaleRevision,
  mergeAccountSnapshot,
  mergeIncomingState
} from './stateMerge.ts'

function account(id, overrides = {}) {
  return {
    id,
    email: `${id}@example.com`,
    accountId: `openai-${id}`,
    subscriptionExpiresAt: null,
    subscriptionPlan: null,
    subscriptionDetectedAt: null,
    subscriptionCheckedAt: null,
    tokenHealth: {
      status: 'unknown',
      lastCheckedAt: null,
      lastRefreshedAt: null,
      lastError: null
    },
    quota: null,
    createdAt: 1000,
    lastLoginAt: 1000,
    issues: { quota: null, subscription: null },
    ...overrides
  }
}

function state(revision, overrides = {}) {
  return {
    revision,
    accounts: [],
    activeAccountId: null,
    appSettings: {
      autoRefreshEnabled: true,
      autoRefreshIntervalMinutes: 15,
      closeToTray: true,
      hiddenSubscriptionCategories: [],
      hiddenAccountIds: []
    },
    ...overrides
  }
}

test('stale revisions are rejected while preserving latest settings overlays', () => {
  const current = state(42, {
    appSettings: {
      ...state(0).appSettings,
      hiddenAccountIds: ['new-id']
    }
  })
  const stale = state(40, {
    accounts: [account('incoming')],
    appSettings: {
      ...state(0).appSettings,
      hiddenAccountIds: ['stale-id']
    }
  })

  assert.equal(isStaleRevision(40, 42), true)
  const merged = mergeIncomingState(current, stale, null)
  assert.equal(merged.revision, 42)
  assert.deepEqual(merged.accounts, [])
  assert.deepEqual(merged.appSettings.hiddenAccountIds, ['new-id'])

  const overlaid = mergeIncomingState(current, stale, {
    ...current.appSettings,
    hiddenAccountIds: ['queued-id']
  })
  assert.deepEqual(overlaid.appSettings.hiddenAccountIds, ['queued-id'])
})

test('newer revisions replace the outer state but preserve account rows by id', () => {
  const current = state(1, {
    accounts: [account('a'), account('b')]
  })
  const incoming = state(2, {
    accounts: [account('b'), account('c')]
  })

  const merged = mergeIncomingState(current, incoming, null)
  assert.equal(merged.revision, 2)
  assert.deepEqual(merged.accounts.map((row) => row.id), ['b', 'c'])
})

test('account snapshots keep the newest quota, subscription, token, and issue data', () => {
  const current = account('a', {
    subscriptionDetectedAt: 100,
    subscriptionCheckedAt: 100,
    subscriptionPlan: 'Plus',
    subscriptionExpiresAt: 500,
    issues: { quota: 'old quota error', subscription: null },
    tokenHealth: {
      status: 'healthy',
      lastCheckedAt: 100,
      lastRefreshedAt: 100,
      lastError: null
    }
  })
  const incoming = account('a', {
    subscriptionDetectedAt: 200,
    subscriptionCheckedAt: 200,
    subscriptionPlan: 'Pro',
    subscriptionExpiresAt: 900,
    issues: { quota: null, subscription: 'subscription warning' },
    tokenHealth: {
      status: 'needs_relogin',
      lastCheckedAt: 100,
      lastRefreshedAt: 100,
      lastError: 'session expired'
    }
  })

  const merged = mergeAccountSnapshot(current, incoming)
  assert.equal(merged.subscriptionPlan, 'Pro')
  assert.equal(merged.subscriptionExpiresAt, 900)
  assert.equal(merged.issues.subscription, 'subscription warning')
  assert.equal(merged.issues.quota, null)
  assert.equal(merged.lastError, undefined)
  assert.equal(merged.issues.quota, null)
  assert.equal(merged.issues.subscription, 'subscription warning')
  assert.equal(merged.tokenHealth.status, 'needs_relogin')
})
