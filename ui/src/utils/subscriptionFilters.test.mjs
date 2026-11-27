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
    [['all', 4], ['plus', 2], ['free', 1], ['pro', 1]]
  )
})

test('Codex Pro tiers have separate badges and filters, including an unspecified tier', () => {
  const tiers = ['Pro 100', 'Pro 200', 'Pro 500', 'Pro']
  const accounts = tiers.map(plan => account(plan, plan))
  assert.deepEqual(accounts.map(a => formatSubscriptionPlan(a.subscriptionPlan)), tiers)
  assert.deepEqual(subscriptionFiltersForAccounts(accounts).slice(1).map(f => f.label), tiers)
  for (const a of accounts) {
    assert.deepEqual(filterAccountsBySubscription(accounts, subscriptionCategoryForAccount(a)), [a])
  }
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

test('Google plan names preserve trial, capacity and enterprise qualifiers', () => {
  for (const [raw, label, category] of [
    ['Google AI Pro (Trial)', 'Google AI Pro (Trial)', 'google-ai-pro'],
    ['Google AI Ultra 30 TB', 'Google AI Ultra 30 TB', 'google-ai-ultra'],
    ['g1-pro-tier', 'Google AI Pro', 'google-ai-pro'],
    ['Google AI Plus', 'Google AI Plus', 'google-ai-plus'],
    ['free-tier', 'Free', 'free'],
    ['Enterprise Premium', 'Enterprise Premium', null],
    ['Google AI Professional', 'Google AI Professional', null],
    ['partner-pro-beta', 'partner-pro-beta', null],
    ['Pro', 'Pro', null],
    [null, null, null]
  ]) {
    assert.equal(formatSubscriptionPlan(raw, 'gemini'), label)
    assert.equal(subscriptionCategoryForAccount(account('test', raw, null, 'gemini')), category)
  }
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

test('tag-only accounts are absent from All, plan tabs and shared tags, including their counts', () => {
  const accounts = ['one', 'two', 'three', 'four'].map(id => account(id, 'Plus'))
  const hidden = ['three']
  const tags = [
    { id: 'tag:work', name: 'Work', accountIds: ['one', 'two', 'three'], showInAll: true, refreshOnlyInTab: true },
    { id: 'tag:archive', name: 'Archive', accountIds: ['two', 'three'], showInAll: false, refreshOnlyInTab: false },
    { id: 'tag:empty', name: 'Empty', accountIds: [], showInAll: true },
    { id: 'tag:second', name: 'Second private tag', accountIds: ['two'], showInAll: false }
  ]
  const filters = subscriptionFiltersForAccounts(accounts, hidden, tags)
  assert.deepEqual(filters.map(f => [f.id, f.count]), [
    ['all', 2], ['plus', 2], ['tag:work', 1], ['tag:archive', 1], ['tag:empty', 0], ['tag:second', 1], ['hidden', 1]
  ])
  const ids = filter => filterAccountsBySubscription(accounts, filter, hidden, tags).map(a => a.id)
  assert.deepEqual(ids('all'), ['one', 'four'])
  assert.deepEqual(ids('plus'), ['one', 'four'])
  assert.deepEqual(ids('tag:work'), ['one'])
  assert.deepEqual(ids('tag:archive'), ['two'])
  assert.deepEqual(ids('tag:second'), ['two'])
  assert.deepEqual(ids('hidden'), ['three'])
  for (const filter of filters) assert.equal(ids(filter.id).length, filter.count)
  tags[1].showInAll = true
  assert.deepEqual(ids('tag:archive'), [])
  tags[3].showInAll = true
  assert.deepEqual(ids('plus'), ['one', 'two', 'four'])
  assert.deepEqual(ids('tag:work'), ['one', 'two'])
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
