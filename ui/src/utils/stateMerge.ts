import type { Account, AppData, AppSettings } from '../types'

// DTOs contain only JSON values. Reuse equal subtrees after IPC deserialization.
export function sameValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true
  if (left == null || right == null || typeof left !== 'object' || typeof right !== 'object') return false
  if (Array.isArray(left) !== Array.isArray(right)) return false
  const keys = Object.keys(left)
  return keys.length === Object.keys(right).length && keys.every((key) =>
    Object.hasOwn(right, key) && sameValue((left as Record<string, unknown>)[key], (right as Record<string, unknown>)[key])
  )
}

function reuseEqual<T>(current: T, incoming: T): T {
  return sameValue(current, incoming) ? current : incoming
}

const NEVER = Number.MIN_SAFE_INTEGER

export function isStaleRevision(
  incoming: number,
  current: number | null | undefined
): boolean {
  return current != null && incoming < current
}

function timestamp(...values: Array<number | null | undefined>): number {
  return Math.max(...values.map((value) => value ?? NEVER))
}

export function mergeAccountSnapshot(
  current: Account,
  incoming: Account
): Account {
  const currentQuotaAt = current.quota?.fetchedAt ?? NEVER
  const incomingQuotaAt = incoming.quota?.fetchedAt ?? NEVER
  const currentTokenAt = timestamp(
    current.tokenHealth.lastCheckedAt,
    current.tokenHealth.lastRefreshedAt
  )
  const incomingTokenAt = timestamp(
    incoming.tokenHealth.lastCheckedAt,
    incoming.tokenHealth.lastRefreshedAt
  )
  const currentSubscriptionAt = timestamp(
    current.subscriptionCheckedAt,
    current.subscriptionDetectedAt
  )
  const incomingSubscriptionAt = timestamp(
    incoming.subscriptionCheckedAt,
    incoming.subscriptionDetectedAt
  )

  const quotaNewest = incomingQuotaAt >= currentQuotaAt
  const tokenNewest = incomingTokenAt >= currentTokenAt
  const subscriptionNewest = incomingSubscriptionAt >= currentSubscriptionAt

  const newEmail = incoming.email ?? current.email
  const newAccountId = incoming.accountId ?? current.accountId
  const newSubscriptionExpiresAt = subscriptionNewest ? incoming.subscriptionExpiresAt : current.subscriptionExpiresAt
  const newSubscriptionPlan = subscriptionNewest ? incoming.subscriptionPlan : current.subscriptionPlan
  const newSubscriptionDetectedAt = subscriptionNewest ? incoming.subscriptionDetectedAt : current.subscriptionDetectedAt
  const newSubscriptionCheckedAt = subscriptionNewest ? incoming.subscriptionCheckedAt : current.subscriptionCheckedAt
  const newTokenHealth = tokenNewest ? reuseEqual(current.tokenHealth, incoming.tokenHealth) : current.tokenHealth
  const newQuota = quotaNewest ? reuseEqual(current.quota, incoming.quota) : current.quota
  const newLastLoginAt = Math.max(current.lastLoginAt, incoming.lastLoginAt)

  const quotaIssue = quotaNewest
    ? incoming.issues?.quota ?? null
    : current.issues?.quota ?? null
  const subIssue = subscriptionNewest
    ? incoming.issues?.subscription ?? null
    : current.issues?.subscription ?? null
  const currentQuotaIssue = current.issues?.quota ?? null
  const currentSubIssue = current.issues?.subscription ?? null

  const issuesEqual = quotaIssue === currentQuotaIssue && subIssue === currentSubIssue

  if (
    newEmail === current.email &&
    newAccountId === current.accountId &&
    newSubscriptionExpiresAt === current.subscriptionExpiresAt &&
    newSubscriptionPlan === current.subscriptionPlan &&
    newSubscriptionDetectedAt === current.subscriptionDetectedAt &&
    newSubscriptionCheckedAt === current.subscriptionCheckedAt &&
    newTokenHealth === current.tokenHealth &&
    newQuota === current.quota &&
    newLastLoginAt === current.lastLoginAt &&
    issuesEqual
  ) {
    return current
  }

  const issues = {
    quota: quotaIssue,
    subscription: subIssue
  }

  return {
    ...current,
    email: newEmail,
    accountId: newAccountId,
    subscriptionExpiresAt: newSubscriptionExpiresAt,
    subscriptionPlan: newSubscriptionPlan,
    subscriptionDetectedAt: newSubscriptionDetectedAt,
    subscriptionCheckedAt: newSubscriptionCheckedAt,
    tokenHealth: newTokenHealth,
    quota: newQuota,
    issues,
    lastLoginAt: newLastLoginAt
  }
}

export function mergeServerAccountsPreservingOrder(
  current: Account[],
  incoming: Account[]
): Account[] {
  const incomingById = new Map(incoming.map((account) => [account.id, account]))
  const currentIds = new Set(current.map((account) => account.id))
  let anyChanged = false
  const retained = current.flatMap((account) => {
    const serverAccount = incomingById.get(account.id)
    if (!serverAccount) {
      anyChanged = true
      return []
    }
    const merged = mergeAccountSnapshot(account, serverAccount)
    if (merged !== account) {
      anyChanged = true
    }
    return [merged]
  })
  const added = incoming.filter((account) => !currentIds.has(account.id))
  if (added.length > 0) {
    anyChanged = true
  }
  if (!anyChanged && retained.length === current.length) {
    return current
  }
  return [...retained, ...added]
}

export function mergeIncomingState(
  current: AppData,
  incoming: AppData,
  settingsOverlay: AppSettings | null
): AppData {
  if (isStaleRevision(incoming.revision, current.revision)) {
    return settingsOverlay ? { ...current, appSettings: settingsOverlay } : current
  }

  const accounts = mergeServerAccountsPreservingOrder(current.accounts, incoming.accounts)
  const appSettings = reuseEqual(current.appSettings, settingsOverlay ?? incoming.appSettings)
  if (accounts === current.accounts && appSettings === current.appSettings
    && incoming.revision === current.revision && incoming.activeAccountId === current.activeAccountId
    && incoming.activeGeminiAccountId === current.activeGeminiAccountId) return current
  return { ...incoming, accounts, appSettings }
}
