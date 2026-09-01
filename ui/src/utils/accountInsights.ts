import type { Account } from '../types'

export function accountRemainingPercent(account: Account): number | null {
  if (!account.quota) return null
  const values = [account.quota.primary, account.quota.secondary]
    .map((window) => window.usedPercent)
    .filter((value): value is number => value != null)
    .map((used) => Math.max(0, Math.min(100, 100 - used)))
  return values.length ? Math.min(...values) : null
}

export function recommendedAccount(accounts: Account[]): Account | null {
  return accounts
    .filter((account) => account.tokenHealth.status !== 'needs_relogin')
    .map((account) => ({ account, remaining: accountRemainingPercent(account) }))
    .filter((item): item is { account: Account; remaining: number } => item.remaining != null)
    .sort((left, right) => right.remaining - left.remaining)[0]?.account ?? null
}
