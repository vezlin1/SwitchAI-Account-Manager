import type { Account, AppData, AppSettings } from '../types'

const NEVER = Number.MIN_SAFE_INTEGER

// IPC deserializes every nested object again. Share equal parts of these plain
// JSON snapshots so a no-op reload does not invalidate memoized account rows.
function shareSnapshot<T>(current: T, incoming: T): T {
  if (Object.is(current, incoming)) return current
  if (
    current == null || incoming == null ||
    typeof current !== 'object' || typeof incoming !== 'object' ||
    Array.isArray(current) !== Array.isArray(incoming)
  ) return incoming

  const previous = current as Record<string, unknown>
  const next = incoming as Record<string, unknown>
  const keys = Object.keys(next)
  let equal = keys.length === Object.keys(previous).length
  const shared = (Array.isArray(incoming) ? [] : {}) as unknown as Record<string, unknown>
  for (const key of keys) {
    const value = shareSnapshot(previous[key], next[key])
    shared[key] = value
    if (!Object.hasOwn(previous, key) || value !== previous[key]) equal = false
  }
  return equal ? current : shared as T
}

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
  const newTokenHealth = tokenNewest
    ? shareSnapshot(current.tokenHealth, incoming.tokenHealth)
    : current.tokenHealth
  const newQuota = quotaNewest
    ? shareSnapshot(current.quota, incoming.quota)
    : current.quota
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

  const issues = issuesEqual ? current.issues : {
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

export function mergeAccountUpdates(current: AppData, updates: Iterable<Account>): AppData {
  const updatesById = new Map<string, Account>()
  for (const update of updates) {
    const previous = updatesById.get(update.id)
    updatesById.set(update.id, previous ? mergeAccountSnapshot(previous, update) : update)
  }
  let accounts: Account[] | undefined
  for (let index = 0; index < current.accounts.length; index += 1) {
    const account = current.accounts[index]
    const update = updatesById.get(account.id)
    if (!update) continue
    const merged = mergeAccountSnapshot(account, update)
    if (merged === account) continue
    accounts ??= current.accounts.slice()
    accounts[index] = merged
  }
  return accounts ? { ...current, accounts } : current
}

export function applyServerAccountOrder(
  current: AppData,
  incoming: AppData,
  reconcile: (current: Account, incoming: Account) => Account = (account) => account
): AppData {
  if (isStaleRevision(incoming.revision, current.revision)) return current

  const currentById = new Map(current.accounts.map((account) => [account.id, account]))
  const incomingIds = new Set<string>()
  const accounts: Account[] = []
  for (const incomingAccount of incoming.accounts) {
    incomingIds.add(incomingAccount.id)
    const currentAccount = currentById.get(incomingAccount.id)
    if (currentAccount) accounts.push(reconcile(currentAccount, incomingAccount))
  }
  for (const account of current.accounts) {
    if (!incomingIds.has(account.id)) accounts.push(account)
  }

  const unchanged = accounts.length === current.accounts.length &&
    accounts.every((account, index) => account === current.accounts[index])
  if (unchanged && current.revision === incoming.revision) return current
  return {
    ...current,
    accounts: unchanged ? current.accounts : accounts,
    revision: incoming.revision
  }
}

function mergeServerAccountsPreservingOrder(
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
    if (!settingsOverlay) return current
    const appSettings = shareSnapshot(current.appSettings, settingsOverlay)
    return appSettings === current.appSettings ? current : { ...current, appSettings }
  }

  const accounts = mergeServerAccountsPreservingOrder(current.accounts, incoming.accounts)
  const appSettings = shareSnapshot(current.appSettings, settingsOverlay ?? incoming.appSettings)
  if (
    incoming.revision === current.revision && accounts === current.accounts &&
    appSettings === current.appSettings && incoming.activeAccountId === current.activeAccountId &&
    incoming.activeGeminiAccountId === current.activeGeminiAccountId
  ) return current

  return {
    ...incoming,
    accounts,
    appSettings
  }
}
