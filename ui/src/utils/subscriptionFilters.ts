import type { Account } from '../types'
import { formatSubscriptionPlan } from './dateUtils.ts'

export type SubscriptionFilterId =
  | 'all'
  | 'plus'
  | 'free'
  | 'pro-x20'
  | 'pro-x5'
  | 'google-ai-pro'
  | 'google-ai-ultra'
  | 'advanced'
  | 'ai-premium'
  | 'workspace'
  | 'developer'

export type SubscriptionCategoryId = Exclude<SubscriptionFilterId, 'all'>

export type SubscriptionFilter = {
  id: SubscriptionFilterId
  label: string
  count: number
}

const FILTER_ORDER: Array<{ id: SubscriptionCategoryId; label: string }> = [
  { id: 'plus', label: 'Plus' },
  { id: 'free', label: 'Free' },
  { id: 'pro-x20', label: 'Pro x20' },
  { id: 'pro-x5', label: 'Pro x5' },
  { id: 'google-ai-pro', label: 'Google AI Pro' },
  { id: 'google-ai-ultra', label: 'Google AI Ultra' },
  { id: 'advanced', label: 'Advanced' },
  { id: 'ai-premium', label: 'AI Premium' },
  { id: 'workspace', label: 'Workspace' },
  { id: 'developer', label: 'Developer' }
]

export function subscriptionCategoryForAccount(account: Account): SubscriptionCategoryId | null {
  const plan = formatSubscriptionPlan(
    account.subscriptionPlan ?? account.quota?.planType,
    account.provider
  )
  if (plan === 'Plus') return 'plus'
  if (plan === 'Free' || plan === 'Free (Restricted)') return 'free'
  if (plan === 'Pro x20' || plan === 'Pro') return 'pro-x20'
  if (plan === 'Pro x5') return 'pro-x5'
  if (plan?.startsWith('Google AI Pro')) return 'google-ai-pro'
  if (plan?.startsWith('Google AI Ultra')) return 'google-ai-ultra'
  if (plan === 'Advanced') return 'advanced'
  if (plan === 'AI Premium') return 'ai-premium'
  if (plan === 'Workspace') return 'workspace'
  if (plan === 'Developer') return 'developer'
  return null
}

export function subscriptionFiltersForAccounts(
  accounts: Account[],
  hiddenCategoriesFromAll: string[] = [],
  hiddenAccountIds: string[] = []
): SubscriptionFilter[] {
  const counts = new Map<SubscriptionCategoryId, number>()
  for (const account of accounts) {
    const category = subscriptionCategoryForAccount(account)
    if (category) counts.set(category, (counts.get(category) ?? 0) + 1)
  }

  const hiddenCategories = new Set(hiddenCategoriesFromAll)
  const hiddenAccounts = new Set(hiddenAccountIds)
  const allCount = accounts.filter((account) => {
    const category = subscriptionCategoryForAccount(account)
    return !hiddenAccounts.has(account.id)
      && (category == null || !hiddenCategories.has(category))
  }).length

  return [
    { id: 'all', label: 'All', count: allCount },
    ...FILTER_ORDER
      .filter(({ id }) => counts.has(id))
      .map(({ id, label }) => ({ id, label, count: counts.get(id) ?? 0 }))
  ]
}

export function filterAccountsBySubscription(
  accounts: Account[],
  filter: SubscriptionFilterId,
  hiddenCategoriesFromAll: string[] = [],
  hiddenAccountIds: string[] = []
): Account[] {
  if (filter === 'all') {
    const hiddenCategories = new Set(hiddenCategoriesFromAll)
    const hiddenAccounts = new Set(hiddenAccountIds)
    return accounts.filter((account) => {
      const category = subscriptionCategoryForAccount(account)
      return !hiddenAccounts.has(account.id)
        && (category == null || !hiddenCategories.has(category))
    })
  }
  return accounts.filter((account) => subscriptionCategoryForAccount(account) === filter)
}
