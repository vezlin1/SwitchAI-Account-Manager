export const isMac = typeof navigator !== 'undefined' && (
  navigator.userAgent.toLowerCase().includes('mac') ||
  (navigator.platform ?? '').toLowerCase().includes('mac')
)
