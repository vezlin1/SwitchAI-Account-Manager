import type { Account } from '../../types'

export type AccountStatusFlags = {
  isSwitching: boolean
  isRemoving: boolean
  isRefreshing: boolean
}

export type AccountRowState = AccountStatusFlags & {
  needsRelogin: boolean
  detectedSubscriptionDate: number | null
}

export function computeAccountStatusFlags(
  accountId: string,
  busyKeys: ReadonlySet<string>,
  refreshingAll: boolean,
  autoRefreshing: boolean
): AccountStatusFlags {
  const isSwitching = busyKeys.has(`switch:${accountId}`)
  const isRemoving = busyKeys.has(`delete:${accountId}`)
  const isRefreshing =
    refreshingAll ||
    autoRefreshing ||
    busyKeys.has(`quota:${accountId}`) ||
    busyKeys.has(`subscription-detect:${accountId}`)

  return { isSwitching, isRemoving, isRefreshing }
}

export function useAccountRowState(
  account: Account,
  statusFlags: AccountStatusFlags
): AccountRowState {
  const needsRelogin = account.tokenHealth?.status === 'needs_relogin'
  const detectedSubscriptionDate = account.subscriptionDetectedAt
    ? account.subscriptionExpiresAt
    : null

  return {
    ...statusFlags,
    needsRelogin,
    detectedSubscriptionDate
  }
}
