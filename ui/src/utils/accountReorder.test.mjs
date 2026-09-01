import assert from 'node:assert/strict'
import test from 'node:test'
import { reorderFilteredAccounts } from './accountReorder.ts'

function acc(id, provider = 'codex', plan = 'Plus') {
  return { id, provider, subscriptionPlan: plan }
}

test('reordering visible accounts preserves slot positions of other providers', () => {
  const c1 = acc('c1', 'codex')
  const g1 = acc('g1', 'gemini')
  const c2 = acc('c2', 'codex')
  const g2 = acc('g2', 'gemini')
  const allAccounts = [c1, g1, c2, g2]

  // On Gemini tab, only g1 and g2 are visible
  const visible = [g1, g2]

  // Drag g2 before g1
  const result = reorderFilteredAccounts(allAccounts, visible, 'g2', 'g1')

  assert.deepEqual(
    result.map((a) => a.id),
    ['c1', 'g2', 'c2', 'g1']
  )
})

test('reordering within a subscription filter preserves slots of other tiers', () => {
  const p1 = acc('p1', 'codex', 'Plus')
  const pro1 = acc('pro1', 'codex', 'Pro x20')
  const p2 = acc('p2', 'codex', 'Plus')
  const pro2 = acc('pro2', 'codex', 'Pro x20')
  const pro3 = acc('pro3', 'codex', 'Pro x20')
  const allAccounts = [p1, pro1, p2, pro2, pro3]

  // Filtered to Pro accounts only
  const visible = [pro1, pro2, pro3]

  // Drag pro3 to before pro1
  const result = reorderFilteredAccounts(allAccounts, visible, 'pro3', 'pro1')

  assert.deepEqual(
    result.map((a) => a.id),
    ['p1', 'pro3', 'p2', 'pro1', 'pro2']
  )
})

test('reordering when all accounts are visible works like standard arrayMove', () => {
  const a1 = acc('a1')
  const a2 = acc('a2')
  const a3 = acc('a3')
  const allAccounts = [a1, a2, a3]

  const result = reorderFilteredAccounts(allAccounts, allAccounts, 'a3', 'a1')
  assert.deepEqual(
    result.map((a) => a.id),
    ['a3', 'a1', 'a2']
  )
})

test('no-op on invalid or identical IDs', () => {
  const a1 = acc('a1')
  const a2 = acc('a2')
  const allAccounts = [a1, a2]

  assert.deepEqual(reorderFilteredAccounts(allAccounts, allAccounts, 'a1', 'a1'), allAccounts)
  assert.deepEqual(reorderFilteredAccounts(allAccounts, allAccounts, 'unknown', 'a1'), allAccounts)
})
