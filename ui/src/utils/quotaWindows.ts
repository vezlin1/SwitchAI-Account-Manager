import type { Account, QuotaInfo, QuotaWindow, QuotaWindows } from '../types'

type QuotaWindowSource = 'primary' | 'secondary'

export type QuotaColumn = {
  key: string
  label: string
  cellTitle: string
  sortSeconds: number
  source: QuotaWindowSource | null
}

const HOUR_SECONDS = 60 * 60
const DAY_SECONDS = 24 * HOUR_SECONDS
const FIVE_HOURS_SECONDS = 5 * HOUR_SECONDS
const WEEK_SECONDS = 7 * DAY_SECONDS
const MIN_MONTH_SECONDS = 27 * DAY_SECONDS
const MAX_MONTH_SECONDS = 32 * DAY_SECONDS
const QUOTA_FRESH_SECONDS = HOUR_SECONDS

export function quotaResetTime(window: QuotaWindow | null, fetchedAt: number | null): number | null {
  if (!window?.resetAt || !Number.isFinite(window.resetAt)) return null
  return window.resetAt > (window.fetchedAt ?? fetchedAt ?? 0) ? window.resetAt : null
}

export function quotaWindowIsStale(
  window: QuotaWindow | null,
  fetchedAt: number | null,
  now = Date.now() / 1000
): boolean {
  if (window?.usedPercent == null) return false
  const checkedAt = window.fetchedAt ?? fetchedAt
  if (checkedAt == null || !Number.isFinite(checkedAt)) return true
  if (!Number.isFinite(window.usedPercent) || window.usedPercent < 0 || window.usedPercent > 100) return true
  const resetAt = quotaResetTime(window, fetchedAt)
  const age = now - checkedAt
  return age < -60 || age >= QUOTA_FRESH_SECONDS || (resetAt !== null && now >= resetAt)
}

export function quotaIsStale(quota: QuotaInfo | null, now = Date.now() / 1000): boolean {
  return quota !== null && (
    quotaWindowIsStale(quota.primary, quota.fetchedAt, now) ||
    quotaWindowIsStale(quota.secondary, quota.fetchedAt, now)
  )
}

function isNear(value: number, target: number, tolerance: number): boolean {
  return Math.abs(value - target) <= tolerance
}

function hasQuotaWindowData(window: QuotaWindow): boolean {
  return window.usedPercent != null
    || window.limitWindowSeconds != null
    || window.resetAt != null
    || window.fetchedAt != null
}

function describeKnownDuration(seconds: number): Pick<QuotaColumn, 'key' | 'label' | 'cellTitle' | 'sortSeconds' | 'source'> {
  if (isNear(seconds, FIVE_HOURS_SECONDS, 5 * 60)) {
    return {
      key: '5h',
      label: '5-hour limit',
      cellTitle: '5h',
      sortSeconds: FIVE_HOURS_SECONDS,
      source: null
    }
  }

  if (isNear(seconds, WEEK_SECONDS, HOUR_SECONDS)) {
    return {
      key: 'weekly',
      label: 'Weekly limit',
      cellTitle: 'Weekly',
      sortSeconds: WEEK_SECONDS,
      source: null
    }
  }

  if (seconds >= MIN_MONTH_SECONDS && seconds <= MAX_MONTH_SECONDS) {
    return {
      key: 'monthly',
      label: 'Monthly limit',
      cellTitle: 'Monthly',
      sortSeconds: 30 * DAY_SECONDS,
      source: null
    }
  }

  const roundedSeconds = Math.round(seconds)
  const durationLabel = roundedSeconds % DAY_SECONDS === 0
    ? `${roundedSeconds / DAY_SECONDS}d`
    : roundedSeconds % HOUR_SECONDS === 0
      ? `${roundedSeconds / HOUR_SECONDS}h`
      : `${Math.max(1, Math.round(roundedSeconds / 60))}m`

  return {
    key: `duration:${roundedSeconds}`,
    label: `${durationLabel} limit`,
    cellTitle: durationLabel,
    sortSeconds: roundedSeconds,
    source: null
  }
}

function describeWindow(window: QuotaWindow, source: QuotaWindowSource): QuotaColumn {
  const seconds = window.limitWindowSeconds
  if (seconds != null && Number.isFinite(seconds) && seconds > 0) {
    return describeKnownDuration(seconds)
  }

  const cellTitle = source === 'primary' ? 'Primary' : 'Secondary'
  return {
    key: `unknown:${source}`,
    label: `${cellTitle} limit`,
    cellTitle,
    sortSeconds: Number.MAX_SAFE_INTEGER + (source === 'secondary' ? 1 : 0),
    source
  }
}

function quotaEntries(quota: QuotaWindows | null | undefined): Array<{
  source: QuotaWindowSource
  window: QuotaWindow
}> {
  if (!quota) return []

  return [
    { source: 'primary', window: quota.primary },
    { source: 'secondary', window: quota.secondary }
  ].filter(({ window }) => hasQuotaWindowData(window)) as Array<{
    source: QuotaWindowSource
    window: QuotaWindow
  }>
}

const antigravityColumns: QuotaColumn[] = [FIVE_HOURS_SECONDS, WEEK_SECONDS].map(describeKnownDuration)

export function quotaColumnsForAccounts(accounts: Account[]): QuotaColumn[] {
  const columns = new Map<string, QuotaColumn>()
  if (accounts.some((account) => account.provider === 'gemini')) {
    for (const column of antigravityColumns) columns.set(column.key, column)
  }

  for (const account of accounts) {
    for (const { source, window } of quotaEntries(account.quota)) {
      // A column is useful when at least one visible account can show an actual
      // quota value. Metadata-only windows would render "Not available" in
      // every row and unnecessarily force horizontal scrolling.
      if (window.usedPercent == null) continue
      const column = describeWindow(window, source)
      if (!columns.has(column.key)) {
        columns.set(column.key, column)
      }
    }
  }

  return [...columns.values()].sort((left, right) => {
    const durationDifference = left.sortSeconds - right.sortSeconds
    return durationDifference || left.label.localeCompare(right.label)
  })
}

export function quotaWindowForColumn(
  quota: QuotaWindows | null | undefined,
  column: QuotaColumn
): QuotaWindow | null {
  for (const { source, window } of quotaEntries(quota)) {
    const candidate = describeWindow(window, source)
    if (candidate.key === column.key) {
      return window
    }
  }

  return null
}

export function quotaCellsForAccount(quota: QuotaWindows | null | undefined, columns: QuotaColumn[], keepMissing = false) {
  const cells = columns.map((column) => ({
    column,
    window: quotaWindowForColumn(quota, column),
    colSpan: 1
  }))
  if (keepMissing) return cells
  const available = cells.filter((cell) => cell.window?.usedPercent != null)
  if (available.length === 0) return cells

  // Give gaps to the preceding available quota, including leading gaps in the first cell.
  let start = 0
  return available.map((cell, index) => {
    const next = available[index + 1]
    const end = next ? cells.indexOf(next) : cells.length
    const colSpan = end - start
    start = end
    return { ...cell, colSpan }
  })
}

export function detailQuotaGroups(account: Account) {
  if (account.provider === 'gemini') {
    return [
      { title: 'Gemini Models', cells: quotaCellsForAccount(account.quota, antigravityColumns, true) },
      { title: 'Claude and GPT models', cells: quotaCellsForAccount(account.quota?.claudeAndGpt, antigravityColumns, true) }
    ]
  }
  const cells = quotaCellsForAccount(account.quota, quotaColumnsForAccounts([account]))
  return cells.length ? [{ title: null, cells }] : []
}

const EXHAUSTED_USED_PERCENT = 99.5

/** The window is used up and its reset is still ahead. */
export function quotaWindowExhausted(
  window: QuotaWindow | null,
  fetchedAt: number | null,
  now = Date.now() / 1000
): boolean {
  if (window?.usedPercent == null || !Number.isFinite(window.usedPercent)) return false
  const resetAt = quotaResetTime(window, fetchedAt)
  return window.usedPercent >= EXHAUSTED_USED_PERCENT && resetAt !== null && resetAt > now
}

/**
 * When a used-up account can work again: the latest reset among its used-up
 * windows, because every one of them has to come back. Null while it still has
 * quota (or nothing is known).
 */
export function accountBackAt(account: Account, now = Date.now() / 1000): number | null {
  const quota = account.quota
  if (!quota) return null
  const resets = [quota.primary, quota.secondary]
    .filter((window) => quotaWindowExhausted(window, quota.fetchedAt, now))
    .map((window) => quotaResetTime(window, quota.fetchedAt))
    .filter((resetAt): resetAt is number => resetAt !== null)
  return resets.length ? Math.max(...resets) : null
}
