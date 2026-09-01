import assert from 'node:assert/strict'
import test from 'node:test'
import { quotaColumnsForAccounts, quotaWindowForColumn } from './quotaWindows.ts'

function window(limitWindowSeconds, usedPercent = 25) {
  return {
    usedPercent,
    limitWindowSeconds,
    resetAt: 2_000_000_000,
    fetchedAt: 1_900_000_000
  }
}

function quota(primary, secondary = null) {
  return {
    planType: null,
    primary,
    secondary: secondary ?? {
      usedPercent: null,
      limitWindowSeconds: null,
      resetAt: null,
      fetchedAt: null
    },
    fetchedAt: 1_900_000_000
  }
}

function account(id, value) {
  return { id, quota: value }
}

test('weekly-only primary window is not labeled as 5h', () => {
  const weekly = quota(window(7 * 24 * 60 * 60))
  const columns = quotaColumnsForAccounts([account('weekly', weekly)])

  assert.deepEqual(columns.map((column) => column.label), ['Weekly quota'])
  assert.equal(quotaWindowForColumn(weekly, columns[0]), weekly.primary)
})

test('standard account keeps separate 5h and weekly columns', () => {
  const standard = quota(window(5 * 60 * 60), window(7 * 24 * 60 * 60))
  const columns = quotaColumnsForAccounts([account('standard', standard)])

  assert.deepEqual(columns.map((column) => column.label), ['5h quota', 'Weekly quota'])
})

test('monthly-only account does not create a 5h column', () => {
  const monthly = quota(window(30 * 24 * 60 * 60))
  const columns = quotaColumnsForAccounts([account('free', monthly)])

  assert.deepEqual(columns.map((column) => column.label), ['Monthly quota'])
})

test('mixed account types share only their actual quota columns', () => {
  const standard = quota(window(5 * 60 * 60), window(7 * 24 * 60 * 60))
  const monthly = quota(window(28 * 24 * 60 * 60))
  const columns = quotaColumnsForAccounts([
    account('standard', standard),
    account('free', monthly)
  ])

  assert.deepEqual(
    columns.map((column) => column.label),
    ['5h quota', 'Weekly quota', 'Monthly quota']
  )
  assert.equal(quotaWindowForColumn(monthly, columns[0]), null)
  assert.equal(quotaWindowForColumn(monthly, columns[2]), monthly.primary)
})

test('legacy windows without duration keep neutral primary and secondary labels', () => {
  const legacy = quota(window(null), window(null, 50))
  const columns = quotaColumnsForAccounts([account('legacy', legacy)])

  assert.deepEqual(columns.map((column) => column.label), ['Primary quota', 'Secondary quota'])
})

test('metadata-only windows are hidden when every value is unavailable', () => {
  const unavailable = quota(window(7 * 24 * 60 * 60, null), window(30 * 24 * 60 * 60, null))

  assert.deepEqual(quotaColumnsForAccounts([account('unavailable', unavailable)]), [])
})
