export const SUBSCRIPTION_LOCALE = 'en-US'

const SUBSCRIPTION_DATE_FORMAT = new Intl.DateTimeFormat(SUBSCRIPTION_LOCALE, {
  day: 'numeric',
  month: 'long',
  year: 'numeric'
})

export function formatSubscriptionDate(unixTsSeconds: number | null | undefined): string {
  if (!unixTsSeconds) return '—'

  const date = new Date(unixTsSeconds * 1000)
  const isExpired = date.getTime() < Date.now()
  const label = isExpired
    ? 'Expired'
    : 'Until'
  return `${label}\u00A0${SUBSCRIPTION_DATE_FORMAT.format(date)}`
}

const GOOGLE_TIER_LABELS = new Map([
  ['free tier', 'Free'],
  ['free', 'Free'],
  ['g1 pro tier', 'Google AI Pro'],
  ['google ai pro', 'Google AI Pro'],
  ['g1 ultra tier', 'Google AI Ultra'],
  ['google ai ultra', 'Google AI Ultra'],
  ['google ai plus', 'Google AI Plus']
])

function formatAntigravityPlan(raw: string): string {
  const qualifierAt = raw.indexOf('(')
  const base = qualifierAt < 0 ? raw : raw.slice(0, qualifierAt)
  const key = base.toLowerCase().replace(/[\s_-]+/g, ' ').trim()
  const label = GOOGLE_TIER_LABELS.get(key)
  if (!label) return raw
  return qualifierAt < 0 ? label : `${label} ${raw.slice(qualifierAt)}`
}

export function formatSubscriptionPlan(
  plan: string | null | undefined,
  provider: 'codex' | 'gemini' = 'codex'
): string | null {
  const raw = plan?.trim()
  if (!raw) return null
  if (provider === 'gemini') return formatAntigravityPlan(raw)

  const normalized = raw.toLowerCase()
  if (normalized.includes('advanced')) return 'Advanced'
  if (normalized.includes('premium')) return 'AI Premium'
  if (normalized.includes('enterprise')) return 'Enterprise'
  if (normalized.includes('business')) return 'Business'
  if (normalized.includes('workspace')) return 'Workspace'
  if (normalized.includes('studio') || normalized.includes('developer')) return 'Developer'
  if (normalized.includes('team')) return 'Team'
  if (normalized === 'pro') return 'Pro'
  if (normalized.includes('plus')) return 'Plus'
  if (normalized.includes('free')) return 'Free'

  return raw
    .replace(/[_-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

export function isTimestampExpired(unixTsSeconds: number | null | undefined): boolean {
  if (!unixTsSeconds) return false
  return unixTsSeconds * 1000 < Date.now()
}

export const STATUS_PREVIEW_LIMIT = 96

function jsonField(raw: string, field: string): string | null {
  const match = raw.match(new RegExp(`"${field}"\\s*:\\s*"([^"]+)"`, 'i'))
  if (!match?.[1]) return null

  try {
    return JSON.parse(`"${match[1]}"`)
  } catch {
    return match[1].replaceAll('\\"', '"')
  }
}

const REGEX_QUOTA_AUTH_FAIL = /^Quota request failed due to auth and token refresh failed:\s*/i
const REGEX_TOKEN_REFRESH_FAIL = /^Token refresh failed before quota request:\s*/i
const REGEX_OAUTH_REFRESH_FAIL = /^OAuth refresh failed \([^)]+\):\s*/i
const REGEX_QUOTA_FAIL = /^Quota request failed \([^)]+\):\s*/i

const STATUS_ERROR_CACHE = new Map<string, string>()
const STATUS_ERROR_CACHE_MAX = 128

function computeReadableStatusError(raw: string): string {
  const lower = raw.toLowerCase()

  if (lower.includes('unsupported_country') || lower.includes('unsupported country') || lower.includes('unsupported region')) {
    return 'unsupported region'
  }

  if (lower.includes('refresh skipped') && lower.includes('vpn')) {
    return 'Refresh skipped (VPN required)'
  }

  if (lower.includes('timed out') || lower.includes('timeout')) {
    return 'Request timed out'
  }

  if (
    lower.includes('connection refused')
    || lower.includes('connection failed')
    || lower.includes('error sending request for url')
  ) {
    return 'Could not reach the provider'
  }

  const structuredMessage =
    jsonField(raw, 'error_description')
    ?? jsonField(raw, 'message')
    ?? jsonField(raw, 'detail')
    ?? jsonField(raw, 'error')?.replaceAll('_', ' ')

  if (structuredMessage) {
    return structuredMessage
  }

  return raw
    .replace(REGEX_QUOTA_AUTH_FAIL, '')
    .replace(REGEX_TOKEN_REFRESH_FAIL, '')
    .replace(REGEX_OAUTH_REFRESH_FAIL, '')
    .replace(REGEX_QUOTA_FAIL, '')
    .slice(0, STATUS_PREVIEW_LIMIT)
}

export function readableStatusError(rawError: string | null | undefined): string | null {
  if (!rawError?.trim()) return null
  const raw = rawError.trim()
  const cached = STATUS_ERROR_CACHE.get(raw)
  if (cached !== undefined) return cached

  const result = computeReadableStatusError(raw)
  if (STATUS_ERROR_CACHE.size >= STATUS_ERROR_CACHE_MAX) {
    const firstKey = STATUS_ERROR_CACHE.keys().next().value
    if (firstKey) STATUS_ERROR_CACHE.delete(firstKey)
  }
  STATUS_ERROR_CACHE.set(raw, result)
  return result
}
