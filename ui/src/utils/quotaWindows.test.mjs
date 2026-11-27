import assert from 'node:assert/strict'
import test from 'node:test'
import { detailQuotaGroups, quotaCellsForAccount, quotaColumnsForAccounts, quotaIsStale, quotaResetTime, quotaWindowForColumn, quotaWindowIsStale } from './quotaWindows.ts'

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

test('the available quota spans all columns when the other quota is missing', () => {
  const standard = quota(window(5 * 60 * 60), window(7 * 24 * 60 * 60))
  const columns = quotaColumnsForAccounts([account('standard', standard)])

  for (const remaining of [standard.primary, standard.secondary]) {
    const cells = quotaCellsForAccount(quota(remaining), columns)
    assert.equal(cells.length, 1)
    assert.equal(cells[0].window, remaining)
    assert.equal(cells[0].colSpan, 2)
  }
  assert.deepEqual(quotaCellsForAccount(standard, columns).map((cell) => cell.colSpan), [1, 1])
  assert.equal(quotaCellsForAccount(null, columns).length, 2)
})

test('zero usage remains available and absorbs metadata-only quota cells', () => {
  const standard = quota(window(5 * 60 * 60), window(7 * 24 * 60 * 60))
  const columns = quotaColumnsForAccounts([account('standard', standard)])
  const partial = quota(window(5 * 60 * 60, null), window(7 * 24 * 60 * 60, 0))
  const cells = quotaCellsForAccount(partial, columns)
  assert.equal(cells.length, 1)
  assert.equal(cells[0].column.key, 'weekly')
  assert.equal(cells[0].colSpan, 2)
})

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

test('a reset after the snapshot makes saved quota stale when it is reached', () => {
  const active = { ...window(18_000), fetchedAt: 1_000, resetAt: 1_100 }
  assert.equal(quotaWindowIsStale(active, null, 1_099), false)
  assert.equal(quotaWindowIsStale(active, null, 1_100), true)
  assert.equal(quotaIsStale(quota(active), 1_100), true)
})

test('an obsolete reset timestamp does not invalidate a newly fetched value', () => {
  const active = { ...window(18_000), fetchedAt: 1_000, resetAt: 900 }
  assert.equal(quotaResetTime(active, null), null)
  assert.equal(quotaWindowIsStale(active, null, 1_001), false)
  assert.equal(quotaWindowIsStale(active, null, 4_600), true)
})

test('Antigravity rows show Gemini and details show both families with independent gaps', () => {
  const value = quota(window(18_000, 0.3288))
  value.claudeAndGpt = quota(window(18_000, 0), window(604_800, 39.59704))
  const account = { provider: 'gemini', quota: value }
  const columns = quotaColumnsForAccounts([account])
  assert.deepEqual(columns.map(col => col.key), ['5h', 'weekly'])
  const row = quotaCellsForAccount(value, columns, true)
  assert.deepEqual(row.map(cell => cell.window), [value.primary, null])
  const [gemini, partners] = detailQuotaGroups(account)
  assert.equal(gemini.title, 'Gemini Models')
  assert.deepEqual(gemini.cells, row)
  assert.equal(partners.title, 'Claude and GPT models')
  assert.equal(partners.cells[1].window, value.claudeAndGpt.secondary)
  assert.ok(detailQuotaGroups({ provider: 'gemini', quota: null }).every(group =>
    group.cells.length === 2 && group.cells.every(cell => cell.window === null)))
})
