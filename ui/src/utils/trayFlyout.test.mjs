import assert from 'node:assert/strict'
import test from 'node:test'
import { accountRemainingPercent, recommendedAccount } from './accountInsights.ts'
import { maskEmail, maskAccountId } from './privacy.ts'

function mockAccount(id, provider, usedPercent, status = 'healthy', email = null, accountId = null) {
  return {
    id,
    provider,
    email: email ?? `${id}@example.com`,
    accountId: accountId ?? `acc-${id}`,
    subscriptionExpiresAt: null,
    subscriptionPlan: 'Pro',
    subscriptionDetectedAt: null,
    tokenHealth: { status, lastCheckedAt: null, lastRefreshedAt: null, lastError: null },
    quota: {
      planType: 'Pro',
      primary: { usedPercent, limitWindowSeconds: 18_000, resetAt: 10_000, fetchedAt: 2_000 },
      secondary: { usedPercent: null, limitWindowSeconds: null, resetAt: null, fetchedAt: null },
      fetchedAt: 2_000
    },
    createdAt: 1,
    lastLoginAt: 1,
    issues: { quota: null, subscription: null }
  }
}

function getFlyoutDisplayName(account, privacyMode) {
  if (privacyMode) {
    if (account.email?.trim()) return maskEmail(account.email)
    if (account.accountId?.trim()) return maskAccountId(account.accountId)
    return '••••••••'
  }
  return account.email?.trim() || account.accountId?.trim() || 'Unnamed account'
}

function getFlyoutQuotaColor(remaining) {
  if (remaining == null) return 'neutral'
  if (remaining > 30) return 'green'
  if (remaining > 10) return 'amber'
  return 'red'
}

test('tray flyout recommendation correctly chooses best account per provider', () => {
  const codex1 = mockAccount('c1', 'codex', 75) // 25% remaining
  const codex2 = mockAccount('c2', 'codex', 15) // 85% remaining
  const gemini1 = mockAccount('g1', 'gemini', 50) // 50% remaining
  const gemini2 = mockAccount('g2', 'gemini', 5, 'needs_relogin') // 95% remaining but needs relogin

  const codexBest = recommendedAccount([codex1, codex2])
  assert.equal(codexBest?.id, 'c2')

  const geminiBest = recommendedAccount([gemini1, gemini2])
  assert.equal(geminiBest?.id, 'g1')
})

test('tray flyout quota color categorizes thresholds accurately', () => {
  assert.equal(getFlyoutQuotaColor(null), 'neutral')
  assert.equal(getFlyoutQuotaColor(100), 'green')
  assert.equal(getFlyoutQuotaColor(31), 'green')
  assert.equal(getFlyoutQuotaColor(30), 'amber')
  assert.equal(getFlyoutQuotaColor(11), 'amber')
  assert.equal(getFlyoutQuotaColor(10), 'red')
  assert.equal(getFlyoutQuotaColor(0), 'red')
})

test('tray flyout account display name honors privacy mode', () => {
  const acc = mockAccount('u1', 'codex', 20, 'healthy', 'john.doe@openai.com', 'user-1234567')

  // Privacy Off
  assert.equal(getFlyoutDisplayName(acc, false), 'john.doe@openai.com')

  // Privacy On
  const masked = getFlyoutDisplayName(acc, true)
  assert.notEqual(masked, 'john.doe@openai.com')
  assert.match(masked, /@openai\.com$/)
  assert.match(masked, /•/)

  // Without email, falls back to masked account ID
  const noEmail = mockAccount('u2', 'codex', 20, 'healthy', '', '12345678')
  assert.equal(getFlyoutDisplayName(noEmail, true), '123••••678')
})

test('tray flyout hero quota formatting avoids displaying 0% left when quota is null', () => {
  const formatHeroRemaining = (account) => {
    const rem = accountRemainingPercent(account)
    return rem != null ? `${Math.round(rem)}% left` : 'Ready'
  }

  const withQuota = mockAccount('q1', 'codex', 25)
  assert.equal(formatHeroRemaining(withQuota), '75% left')

  const withoutQuota = mockAccount('q2', 'codex', null)
  withoutQuota.quota = null
  assert.equal(formatHeroRemaining(withoutQuota), 'Ready')
})

test('tray flyout reset timer selects nearest future reset timestamp', () => {
  const nowSeconds = Math.floor(Date.now() / 1000)
  const pastReset = nowSeconds - 3600
  const futureSoon = nowSeconds + 1800
  const futureLater = nowSeconds + 7200

  const pickNextReset = (account) => {
    const resets = [account.quota?.primary?.resetAt, account.quota?.secondary?.resetAt]
      .filter((ts) => ts != null && ts * 1000 > Date.now())
    resets.sort((a, b) => a - b)
    return resets[0] ?? account.quota?.primary?.resetAt ?? account.quota?.secondary?.resetAt
  }

  const acc = mockAccount('r1', 'codex', 50)
  // Primary is in the past, secondary is in future
  acc.quota.primary.resetAt = pastReset
  acc.quota.secondary.resetAt = futureSoon
  assert.equal(pickNextReset(acc), futureSoon)

  // Both in future, picks the earlier one
  acc.quota.primary.resetAt = futureLater
  acc.quota.secondary.resetAt = futureSoon
  assert.equal(pickNextReset(acc), futureSoon)
})

