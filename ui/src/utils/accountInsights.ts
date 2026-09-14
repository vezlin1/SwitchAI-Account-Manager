import type { Account } from '../types'

export const QUOTA_FRESH_SECONDS = 60 * 60

export function quotaIsFresh(account: Account, now = Date.now() / 1000): boolean {
  const quota = account.quota
  if (!quota || !Number.isFinite(quota.fetchedAt)) return false
  const age = now - quota.fetchedAt
  if (age < -60 || age >= QUOTA_FRESH_SECONDS) return false
  if ([quota.primary, quota.secondary].some((window) => window.usedPercent != null
    && (!Number.isFinite(window.usedPercent) || window.usedPercent < 0 || window.usedPercent > 100))) return false
  return ![quota.primary, quota.secondary].some((window) =>
    window.usedPercent != null && window.resetAt != null
      && window.resetAt <= now && window.resetAt > (window.fetchedAt ?? quota.fetchedAt)
  )
}

export function accountRemainingPercent(account: Account): number | null {
  if (!account.quota) return null
  const values = [account.quota.primary, account.quota.secondary]
    .map((window) => window.usedPercent)
    .filter((value): value is number => value != null && Number.isFinite(value) && value >= 0 && value <= 100)
    .map((used) => Math.max(0, Math.min(100, 100 - used)))
  return values.length ? Math.min(...values) : null
}

export function recommendedAccount(
  accounts: Account[],
  hiddenAccountIds: string[] = [],
  now = Date.now() / 1000
): Account | null {
  const hiddenSet = new Set(hiddenAccountIds)
  return accounts
    .filter((account) => !hiddenSet.has(account.id))
    .filter((account) => ['healthy', 'refreshed'].includes(account.tokenHealth.status))
    .filter((account) => !account.issues?.quota && quotaIsFresh(account, now))
    .map((account) => ({ account, remaining: accountRemainingPercent(account) }))
    .filter((item): item is { account: Account; remaining: number } => item.remaining != null)
    .sort((left, right) => right.remaining - left.remaining)[0]?.account ?? null
}
