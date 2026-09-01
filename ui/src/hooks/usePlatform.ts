export type Platform = 'macos' | 'windows' | 'linux'

export interface PlatformInfo {
  isMac: boolean
  isWindows: boolean
  isLinux: boolean
  platform: Platform
}

function detectPlatform(): Platform {
  if (typeof navigator === 'undefined') return 'windows'
  const ua = navigator.userAgent.toLowerCase()
  const p = (navigator.platform ?? '').toLowerCase()
  if (ua.includes('mac') || p.includes('mac')) return 'macos'
  if (ua.includes('linux') || p.includes('linux')) return 'linux'
  return 'windows'
}

const currentPlatform = detectPlatform()

export function usePlatform(): PlatformInfo {
  return {
    isMac: currentPlatform === 'macos',
    isWindows: currentPlatform === 'windows',
    isLinux: currentPlatform === 'linux',
    platform: currentPlatform
  }
}
