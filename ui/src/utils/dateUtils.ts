export const SUBSCRIPTION_LOCALE = new Intl.DateTimeFormat().resolvedOptions().locale

const SUBSCRIPTION_DATE_FORMAT = new Intl.DateTimeFormat(SUBSCRIPTION_LOCALE, {
  day: 'numeric',
  month: 'long',
  year: 'numeric'
})

const UNTIL_LABELS: Record<string, string> = {
  ar: 'حتى',
  be: 'Да',
  bg: 'До',
  cs: 'Do',
  da: 'Indtil',
  de: 'Bis',
  el: 'Έως',
  en: 'Until',
  es: 'Hasta el',
  fi: 'Voimassa asti',
  fr: 'Jusqu’au',
  he: 'עד',
  hr: 'Do',
  hu: 'Lejárat:',
  id: 'Hingga',
  it: 'Fino al',
  ja: '期限:',
  ko: '만료:',
  nl: 'Tot',
  no: 'Til',
  pl: 'Do',
  pt: 'Até',
  ro: 'Până la',
  ru: 'До',
  sk: 'Do',
  sr: 'Do',
  sv: 'Till',
  th: 'ถึง',
  tr: 'Bitiş:',
  uk: 'До',
  vi: 'Đến',
  zh: '截至'
}

const EXPIRED_LABELS: Record<string, string> = {
  ar: 'منتهي',
  be: 'Скончылася',
  bg: 'Изтекла',
  cs: 'Vypršelo',
  da: 'Udløbet',
  de: 'Abgelaufen',
  el: 'Έληξε',
  en: 'Expired',
  es: 'Expirado',
  fi: 'Vanhentunut',
  fr: 'Expiré',
  he: 'פג תוקף',
  hr: 'Isteklo',
  hu: 'Lejárt',
  id: 'Kedaluwarsa',
  it: 'Scaduto',
  ja: '期限切れ:',
  ko: '만료됨:',
  nl: 'Verlopen',
  no: 'Utløpt',
  pl: 'Wygasła',
  pt: 'Expirado',
  ro: 'Expirat',
  ru: 'Истекла',
  sk: 'Vypršala',
  sr: 'Isteklo',
  sv: 'Utgått',
  th: 'หมดอายุ',
  tr: 'Süresi doldu',
  uk: 'Закінчилась',
  vi: 'Đã hết hạn',
  zh: '已过期'
}

function subscriptionUntilLabel(locale: string): string {
  const language = locale.split('-')[0]?.toLowerCase() ?? 'en'
  return UNTIL_LABELS[language] ?? UNTIL_LABELS.en
}

function subscriptionExpiredLabel(locale: string): string {
  const language = locale.split('-')[0]?.toLowerCase() ?? 'en'
  return EXPIRED_LABELS[language] ?? EXPIRED_LABELS.en
}

export function formatSubscriptionDate(unixTsSeconds: number | null | undefined): string {
  if (!unixTsSeconds) return '—'

  const date = new Date(unixTsSeconds * 1000)
  const isExpired = date.getTime() < Date.now()
  const label = isExpired
    ? subscriptionExpiredLabel(SUBSCRIPTION_LOCALE)
    : subscriptionUntilLabel(SUBSCRIPTION_LOCALE)
  return `${label}\u00A0${SUBSCRIPTION_DATE_FORMAT.format(date)}`
}

function formatAntigravityPlan(raw: string): string {
  const cleaned = raw
    .replace(/[_-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
  const normalized = cleaned.toLowerCase()
  const restricted = normalized.includes('restricted') ? ' (Restricted)' : ''
  const tierName = normalized.replace(/\s*\(restricted\)\s*/g, '').trim()
  const tierTokens = new Set(tierName.split(/\s+/))

  if (tierTokens.has('ultra')) return `Google AI Ultra${restricted}`
  if (tierTokens.has('pro') || tierTokens.has('premium')) return `Google AI Pro${restricted}`
  if (tierTokens.has('free')) return `Free${restricted}`

  // Unknown tiers are kept verbatim: the Cloud Code response is the source of truth.
  return cleaned
}

export function formatSubscriptionPlan(
  plan: string | null | undefined,
  provider: 'codex' | 'gemini' = 'codex'
): string | null {
  const raw = plan?.trim()
  if (!raw) return null
  if (provider === 'gemini') return formatAntigravityPlan(raw)

  const normalized = raw.toLowerCase()
  const compact = normalized.replace(/[\s_-]+/g, '')
  if (normalized.includes('advanced')) return 'Advanced'
  if (normalized.includes('premium')) return 'AI Premium'
  if (normalized.includes('enterprise')) return 'Enterprise'
  if (normalized.includes('business')) return 'Business'
  if (normalized.includes('workspace')) return 'Workspace'
  if (normalized.includes('studio') || normalized.includes('developer')) return 'Developer'
  if (normalized.includes('team')) return 'Team'
  if (normalized.includes('pro')) {
    if (compact.includes('x5') || compact.includes('5x')) return 'Pro x5'
    if (compact.includes('x20') || compact.includes('20x')) return 'Pro x20'
    return 'Pro'
  }
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

  if (lower.includes('refresh skipped')) {
    return 'refresh skipped (VPN required)'
  }

  if (
    lower.includes('timed out')
    || lower.includes('connection refused')
    || lower.includes('connection failed')
    || lower.includes('error sending request for url')
  ) {
    return 'connection blocked / VPN required'
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
