import type { Account, AccountTag } from '../types'
import { formatSubscriptionPlan } from './dateUtils.ts'

export type SubscriptionFilterId =
  | AccountTag['id']
  | 'all'
  | 'hidden'
  | 'plus'
  | 'free'
  | 'pro-x20'
  | 'pro-x5'
  | 'pro-500'
  | 'pro'
  | 'google-ai-plus'
  | 'google-ai-pro'
  | 'google-ai-ultra'
  | 'advanced'
  | 'ai-premium'
  | 'workspace'
  | 'developer'

export type SubscriptionCategoryId = Exclude<SubscriptionFilterId, 'all' | 'hidden' | AccountTag['id']>

export type SubscriptionFilter = {
  id: SubscriptionFilterId
  label: string
  count: number
}

const FILTER_ORDER: Array<{ id: SubscriptionCategoryId; label: string }> = [
  { id: 'plus', label: 'Plus' },
  { id: 'free', label: 'Free' },
  { id: 'pro-x5', label: 'Pro 100' },
  { id: 'pro-x20', label: 'Pro 200' },
  { id: 'pro-500', label: 'Pro 500' },
  { id: 'pro', label: 'Pro' },
  { id: 'google-ai-plus', label: 'Google AI Plus' },
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
  if (account.provider === 'gemini') {
    if (!plan) return null
    if (/^Google AI Pro(?:$|[ (])/i.test(plan)) return 'google-ai-pro'
    if (/^Google AI Ultra(?:$|[ (])/i.test(plan)) return 'google-ai-ultra'
    if (/^Google AI Plus(?:$|[ (])/i.test(plan)) return 'google-ai-plus'
    if (plan === 'Free' || plan === 'Free (Restricted)') return 'free'
    return null
  }
  if (plan === 'Plus') return 'plus'
  if (plan === 'Free' || plan === 'Free (Restricted)') return 'free'
  if (plan === 'Pro 100') return 'pro-x5'
  if (plan === 'Pro 200') return 'pro-x20'
  if (plan === 'Pro 500') return 'pro-500'
  if (plan === 'Pro') return 'pro'
  if (plan === 'Advanced') return 'advanced'
  if (plan === 'AI Premium') return 'ai-premium'
  if (plan === 'Workspace') return 'workspace'
  if (plan === 'Developer') return 'developer'
  return null
}

function groupVisibleAccounts(accounts: Account[], hiddenAccountIds: string[], tags: AccountTag[]) {
  const hiddenIds = new Set(hiddenAccountIds)
  const tagOnlyIds = new Set(tags.filter(tag => !tag.showInAll).flatMap(tag => tag.accountIds))
  const visible = accounts.filter(account => !hiddenIds.has(account.id))
  const shared = visible.filter(account => !tagOnlyIds.has(account.id))
  return { visible, shared }
}

function visibleTagMembers(tag: AccountTag, visible: Account[], shared: Account[]) {
  const members = new Set(tag.accountIds)
  return (tag.showInAll ? shared : visible).filter(account => members.has(account.id))
}

export function subscriptionFiltersForAccounts(
  accounts: Account[],
  hiddenAccountIds: string[] = [],
  tags: AccountTag[] = []
): SubscriptionFilter[] {
  const { visible, shared } = groupVisibleAccounts(accounts, hiddenAccountIds, tags)
  const counts = new Map<SubscriptionCategoryId, number>()
  for (const account of shared) {
    const category = subscriptionCategoryForAccount(account)
    if (category) counts.set(category, (counts.get(category) ?? 0) + 1)
  }

  const hiddenCount = accounts.length - visible.length

  const filtersList: SubscriptionFilter[] = [
    { id: 'all', label: 'All', count: shared.length },
    ...FILTER_ORDER
      .filter(({ id }) => counts.has(id))
      .map(({ id, label }) => ({ id, label, count: counts.get(id) ?? 0 }))
  ]

  for (const tag of tags) {
    const count = visibleTagMembers(tag, visible, shared).length
    filtersList.push({ id: tag.id, label: tag.name, count })
  }

  if (hiddenCount > 0) {
    filtersList.push({ id: 'hidden', label: 'Hidden', count: hiddenCount })
  }

  return filtersList
}

export function filterAccountsBySubscription(
  accounts: Account[],
  filter: SubscriptionFilterId,
  hiddenAccountIds: string[] = [],
  tags: AccountTag[] = []
): Account[] {
  if (filter === 'hidden') {
    const hiddenIds = new Set(hiddenAccountIds)
    return accounts.filter(account => hiddenIds.has(account.id))
  }
  const { visible, shared } = groupVisibleAccounts(accounts, hiddenAccountIds, tags)
  if (filter === 'all') return shared
  if (filter.startsWith('tag:')) {
    const tag = tags.find(tag => tag.id === filter)
    return tag ? visibleTagMembers(tag, visible, shared) : []
  }
  return shared.filter(account => subscriptionCategoryForAccount(account) === filter)
}
