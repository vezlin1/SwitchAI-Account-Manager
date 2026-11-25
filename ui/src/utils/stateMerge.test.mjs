import assert from 'node:assert/strict'
import test from 'node:test'
import {
  applyServerAccountOrder,
  isStaleRevision,
  mergeAccountSnapshot,
  mergeAccountUpdates,
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

test('identical IPC snapshots preserve state and account references', () => {
  const current = state(12, { accounts: [account('one')] })
  const cloned = structuredClone(current)
  assert.equal(mergeIncomingState(current, cloned, null), current)
  cloned.revision = 13
  const newer = mergeIncomingState(current, cloned, null)
  assert.equal(newer.accounts, current.accounts)
  assert.equal(newer.appSettings, current.appSettings)
  assert.equal(newer.revision, 13)
})

test('only changed rows are replaced when a complete snapshot arrives', () => {
  const current = state(12, { accounts: [account('one'), account('two')] })
  const next = structuredClone(current)
  next.revision = 13
  next.accounts[1].tokenHealth = { ...next.accounts[1].tokenHealth, status: 'healthy', lastCheckedAt: 2000 }
  const merged = mergeIncomingState(current, next, null)
  assert.equal(merged.accounts[0], current.accounts[0])
  assert.notEqual(merged.accounts[1], current.accounts[1])
  assert.equal(merged.accounts[1].tokenHealth.status, 'healthy')
})

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

test('order acknowledgements reuse local rows and preserve concurrent account changes', () => {
  const a = account('a')
  const b = account('b', { email: 'new@example.com' })
  const local = account('local')
  const current = state(3, { accounts: [a, b, local], activeAccountId: 'b' })
  const incoming = state(4, { accounts: [account('removed'), account('b'), account('a')] })

  const reordered = applyServerAccountOrder(current, incoming)
  assert.deepEqual(reordered.accounts.map((row) => row.id), ['b', 'a', 'local'])
  assert.equal(reordered.accounts[0], b)
  assert.equal(reordered.accounts[2], local)
  assert.equal(reordered.activeAccountId, 'b')
  assert.equal(reordered.appSettings, current.appSettings)
  assert.equal(reordered.revision, 4)
  assert.equal(applyServerAccountOrder(reordered, incoming), reordered)
})

test('order recovery merges refreshed fields and ignores stale revisions', () => {
  const b = account('b', { subscriptionPlan: 'Free', subscriptionCheckedAt: 100 })
  const current = state(3, { accounts: [account('a'), b] })
  const incoming = state(4, {
    accounts: [account('b', { subscriptionPlan: 'Plus', subscriptionCheckedAt: 200 }), account('a')]
  })

  const recovered = applyServerAccountOrder(current, incoming, mergeAccountSnapshot)
  assert.equal(recovered.accounts[0].subscriptionPlan, 'Plus')
  assert.equal(recovered.accounts[1], current.accounts[0])
  assert.equal(applyServerAccountOrder(recovered, current, mergeAccountSnapshot), recovered)
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

function quota(fetchedAt, usedPercent = 20) {
  return {
    planType: 'Plus',
    fetchedAt,
    primary: { usedPercent, limitWindowSeconds: 18000, resetAt: 900, fetchedAt },
    secondary: { usedPercent: 30, limitWindowSeconds: 604800, resetAt: 1500, fetchedAt }
  }
}

const roundTrip = (value) => JSON.parse(JSON.stringify(value))

test('identical IPC reloads retain outer state, account rows, and nested snapshot identities', () => {
  const current = state(10, {
    accounts: [account('a', { quota: quota(100) }), account('b')],
    appSettings: { ...state(0).appSettings, hiddenAccountIds: ['b'] }
  })
  const merged = mergeIncomingState(current, roundTrip(current), null)
  assert.equal(merged, current)
  assert.equal(merged.accounts, current.accounts)
  assert.equal(merged.accounts[0].quota, current.accounts[0].quota)
  assert.equal(merged.accounts[0].tokenHealth, current.accounts[0].tokenHealth)
  assert.equal(merged.accounts[0].issues, current.accounts[0].issues)
})

test('a real update changes only the affected account and nested quota window', () => {
  const current = state(10, { accounts: [account('a', { quota: quota(100) }), account('b')] })
  const update = roundTrip(current.accounts[0])
  update.quota.primary.usedPercent = 80
  const merged = mergeAccountUpdates(current, [update])
  assert.notEqual(merged, current)
  assert.notEqual(merged.accounts[0], current.accounts[0])
  assert.equal(merged.accounts[1], current.accounts[1])
  assert.equal(merged.accounts[0].quota.primary.usedPercent, 80)
  assert.equal(merged.accounts[0].quota.secondary, current.accounts[0].quota.secondary)
  assert.equal(merged.accounts[0].tokenHealth, current.accounts[0].tokenHealth)
  assert.equal(merged.accounts[0].issues, current.accounts[0].issues)
})

test('equal account events and events for removed accounts are no-op state updates', () => {
  const current = state(10, { accounts: [account('a', { quota: quota(100) })] })
  assert.equal(mergeAccountUpdates(current, [roundTrip(current.accounts[0]), account('removed')]), current)
})

test('batched out-of-order snapshots keep independently newest quota and token data', () => {
  const current = state(10, { accounts: [account('a', { quota: quota(50) })] })
  const quotaUpdate = account('a', { quota: quota(300, 90), issues: { quota: 'quota warning', subscription: null } })
  const tokenUpdate = account('a', {
    quota: quota(100),
    tokenHealth: { status: 'healthy', lastCheckedAt: 400, lastRefreshedAt: null, lastError: null }
  })
  const merged = mergeAccountUpdates(current, [quotaUpdate, tokenUpdate])
  assert.equal(merged.accounts[0].quota.primary.usedPercent, 90)
  assert.equal(merged.accounts[0].quota.fetchedAt, 300)
  assert.equal(merged.accounts[0].tokenHealth.lastCheckedAt, 400)
  assert.equal(merged.accounts[0].issues.quota, 'quota warning')
})

test('settings updates retain unchanged arrays and revisions do not invalidate unchanged rows', () => {
  const current = state(10, { accounts: [account('a')], appSettings: { ...state(0).appSettings, hiddenAccountIds: ['a'] } })
  const incoming = roundTrip(current)
  incoming.revision = 11
  incoming.appSettings.closeToTray = false
  const merged = mergeIncomingState(current, incoming, null)
  assert.equal(merged.revision, 11)
  assert.equal(merged.appSettings.closeToTray, false)
  assert.equal(merged.accounts, current.accounts)
  assert.equal(merged.appSettings.hiddenAccountIds, current.appSettings.hiddenAccountIds)
  assert.equal(merged.appSettings.hiddenSubscriptionCategories, current.appSettings.hiddenSubscriptionCategories)
  assert.equal(mergeIncomingState(current, state(9), roundTrip(current.appSettings)), current)
})

test('newer revisions preserve local account ordering and identities across reordered JSON', () => {
  const current = state(10, { accounts: [account('a'), account('b')] })
  const incoming = roundTrip(current)
  incoming.revision = 11
  incoming.accounts.reverse()
  const merged = mergeIncomingState(current, incoming, null)
  assert.equal(merged.accounts, current.accounts)
  assert.deepEqual(merged.accounts.map((row) => row.id), ['a', 'b'])
})

test('equal timestamp changes to token health and issues remain visible', () => {
  const current = account('a', { quota: quota(100) })
  const incoming = roundTrip(current)
  incoming.tokenHealth.lastError = 'session expired'
  incoming.tokenHealth.status = 'needs_relogin'
  incoming.issues.quota = 'quota failed'
  const merged = mergeAccountSnapshot(current, incoming)
  assert.equal(merged.quota, current.quota)
  assert.equal(merged.tokenHealth.status, 'needs_relogin')
  assert.equal(merged.tokenHealth.lastError, 'session expired')
  assert.equal(merged.issues.quota, 'quota failed')
})
