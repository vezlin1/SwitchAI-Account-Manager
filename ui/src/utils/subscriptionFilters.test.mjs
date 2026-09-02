import assert from 'node:assert/strict'
import test from 'node:test'
import {
  filterAccountsBySubscription,
  subscriptionCategoryForAccount,
  subscriptionFiltersForAccounts
} from './subscriptionFilters.ts'
import { formatSubscriptionPlan } from './dateUtils.ts'
import { quotaColumnsForAccounts } from './quotaWindows.ts'

function account(id, plan, quotaPlan = null, provider = 'codex') {
  return {
    id,
    provider,
    subscriptionPlan: plan,
    quota: quotaPlan ? { planType: quotaPlan } : null
  }
}

test('only subscription categories represented by accounts are returned', () => {
  const filters = subscriptionFiltersForAccounts([
    account('plus-1', 'Plus'),
    account('plus-2', 'chatgpt_plus_plan'),
    account('free', null, 'free'),
    account('pro', 'pro')
  ])

  assert.deepEqual(
    filters.map(({ id, count }) => [id, count]),
    [['all', 4], ['plus', 2], ['free', 1], ['pro-x20', 1]]
  )
})

test('Codex pro multipliers are kept distinct and matching is tolerant', () => {
  assert.equal(subscriptionCategoryForAccount(account('x5', 'chatgpt_pro_5x_plan')), 'pro-x5')
  assert.equal(subscriptionCategoryForAccount(account('x20', 'Pro x20')), 'pro-x20')
  assert.equal(subscriptionCategoryForAccount(account('generic', 'Pro')), 'pro-x20')
})

test('Antigravity tiers use official Google names and never Codex multipliers', () => {
  const pro = account('google-pro', 'Google AI Pro', null, 'gemini')
  const ultra = account('google-ultra', 'g1-ultra-tier', null, 'gemini')
  const restricted = account('google-restricted', 'GOOGLE_AI_PRO (Restricted)', null, 'gemini')

  assert.equal(formatSubscriptionPlan(pro.subscriptionPlan, pro.provider), 'Google AI Pro')
  assert.equal(formatSubscriptionPlan(ultra.subscriptionPlan, ultra.provider), 'Google AI Ultra')
  assert.equal(formatSubscriptionPlan(restricted.subscriptionPlan, restricted.provider), 'Google AI Pro (Restricted)')
  assert.equal(subscriptionCategoryForAccount(pro), 'google-ai-pro')
  assert.equal(subscriptionCategoryForAccount(ultra), 'google-ai-ultra')
  assert.equal(subscriptionCategoryForAccount(restricted), 'google-ai-pro')
  assert.deepEqual(
    subscriptionFiltersForAccounts([pro, ultra, restricted]).map(({ id, count }) => [id, count]),
    [['all', 3], ['google-ai-pro', 2], ['google-ai-ultra', 1]]
  )
})

test('filtering preserves original account order', () => {
  const accounts = [
    account('first', 'Free'),
    account('second', 'Plus'),
    account('third', 'Free')
  ]

  assert.deepEqual(
    filterAccountsBySubscription(accounts, 'free').map(({ id }) => id),
    ['first', 'third']
  )
})

test('hidden accounts are excluded from all regular tabs and appear only in Hidden tab', () => {
  const accounts = [
    account('plus-1', 'Plus'),
    account('plus-2', 'Plus'),
    account('free-1', 'Free')
  ]
  const hiddenIds = ['plus-1']
  const filters = subscriptionFiltersForAccounts(accounts, hiddenIds)

  assert.deepEqual(
    filters.map(({ id, count }) => [id, count]),
    [['all', 2], ['plus', 1], ['free', 1], ['hidden', 1]]
  )
  assert.deepEqual(
    filterAccountsBySubscription(accounts, 'all', hiddenIds).map(({ id }) => id),
    ['plus-2', 'free-1']
  )
  assert.deepEqual(
    filterAccountsBySubscription(accounts, 'plus', hiddenIds).map(({ id }) => id),
    ['plus-2']
  )
  assert.deepEqual(
    filterAccountsBySubscription(accounts, 'hidden', hiddenIds).map(({ id }) => id),
    ['plus-1']
  )
})
