import type { Account, QuotaInfo, QuotaWindow } from '../types'

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
      label: '5h quota',
      cellTitle: '5h',
      sortSeconds: FIVE_HOURS_SECONDS,
      source: null
    }
  }

  if (isNear(seconds, WEEK_SECONDS, HOUR_SECONDS)) {
    return {
      key: 'weekly',
      label: 'Weekly quota',
      cellTitle: 'Weekly',
      sortSeconds: WEEK_SECONDS,
      source: null
    }
  }

  if (seconds >= MIN_MONTH_SECONDS && seconds <= MAX_MONTH_SECONDS) {
    return {
      key: 'monthly',
      label: 'Monthly quota',
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
    label: `${durationLabel} quota`,
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
    label: `${cellTitle} quota`,
    cellTitle,
    sortSeconds: Number.MAX_SAFE_INTEGER + (source === 'secondary' ? 1 : 0),
    source
  }
}

function quotaEntries(quota: QuotaInfo | null | undefined): Array<{
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

export function quotaColumnsForAccounts(accounts: Account[]): QuotaColumn[] {
  const columns = new Map<string, QuotaColumn>()

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
  quota: QuotaInfo | null | undefined,
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
