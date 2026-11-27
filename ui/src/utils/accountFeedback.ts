import type { Account, CommandWarning } from '../types'

export type AccountRefreshNotice = {
  message: string
  failed: boolean
  area: 'quota' | 'subscription'
}

export type AccountAttention = {
  kind: 'verify' | 'relogin' | 'quota' | 'subscription' | 'notice'
  message: string | null
}

type RefreshWarning = Pick<AccountRefreshNotice, 'message' | 'failed'>

export function splitRefreshWarnings(
  warnings: readonly CommandWarning[],
  fallbackAccountId?: string
): { byAccount: Map<string, RefreshWarning>; general: string[] } {
  const byAccount = new Map<string, RefreshWarning>()
  const general: string[] = []
  for (const warning of warnings) {
    const message = warning.message.trim() || 'Refresh could not complete'
    const accountId = warning.accountId ?? fallbackAccountId
    if (!accountId) {
      general.push(message)
      continue
    }
    const previous = byAccount.get(accountId)
    const failed = warning.code === 'refresh_failed' || warning.code === 'quota_refresh_warning'
      || warning.code === 'antigravity_quota_warning' || warning.code === 'subscription_refresh_warning'
    byAccount.set(accountId, {
      message: previous ? `${previous.message} · ${message}` : message,
      failed: failed || (previous?.failed ?? false)
    })
  }
  return { byAccount, general }
}

export function accountAttention(account: Account, notice?: AccountRefreshNotice): AccountAttention | null {
  const quotaIssue = account.issues?.quota
  const authIssue = account.tokenHealth.lastError
  const quotaNotice = notice?.area === 'quota' ? notice.message : null
  const quotaMessage = quotaIssue || authIssue || quotaNotice || null

  if (account.provider === 'gemini') {
    const verificationMessage = [quotaIssue, authIssue, quotaNotice].find((message) =>
      /\b(?:verify your (?:google )?account|account verification required|validation_required)\b/i.test(message ?? '')
    )
    if (verificationMessage) return { kind: 'verify', message: verificationMessage }
  }
  if (account.tokenHealth.status === 'needs_relogin') {
    return { kind: 'relogin', message: authIssue || quotaMessage }
  }
  if (quotaIssue || (quotaNotice && notice?.failed) || account.tokenHealth.status === 'network_error' || account.tokenHealth.status === 'server_error') {
    return { kind: 'quota', message: quotaMessage }
  }
  const subscriptionIssue = account.issues?.subscription || (notice?.area === 'subscription' ? notice.message : null)
  if (subscriptionIssue) return { kind: 'subscription', message: subscriptionIssue }
  return quotaNotice ? { kind: 'notice', message: quotaNotice } : null
}

export function quotaUpdateBlocked(attention: AccountAttention | null): boolean {
  return attention?.kind === 'verify' || attention?.kind === 'relogin' || attention?.kind === 'quota'
}

export function recordedAccountStatus(account: Account): {
  tone: 'healthy' | 'neutral'
  label: string
  message: string
} {
  if (account.tokenHealth.status === 'healthy' || account.tokenHealth.status === 'refreshed') {
    return { tone: 'healthy', label: 'Healthy', message: 'The last recorded connection check succeeded.' }
  }
  if (!account.quota) {
    return { tone: 'neutral', label: 'No quota yet', message: 'This account has no saved quota data.' }
  }
  return { tone: 'neutral', label: 'Not checked', message: 'This account has no recorded connection check.' }
}
