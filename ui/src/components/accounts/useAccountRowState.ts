import type { Account } from '../../types'

export type AccountStatusFlags = {
  isSwitching: boolean
  isRemoving: boolean
  isRelogining: boolean
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
  const isRelogining = busyKeys.has(`relogin:${accountId}`)
  const isRefreshing =
    refreshingAll ||
    autoRefreshing ||
    busyKeys.has('refresh') ||
    busyKeys.has(`quota:${accountId}`) ||
    busyKeys.has(`subscription-detect:${accountId}`) ||
    busyKeys.has(`relogin:${accountId}`) ||
    busyKeys.has(`account:${accountId}:quota`) ||
    busyKeys.has(`account:${accountId}:subscription`)

  return { isSwitching, isRemoving, isRelogining, isRefreshing }
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
