import { api } from '../api'
import type { AntigravitySurface } from '../types'

const ANTIGRAVITY_SURFACES_CACHE_KEY = 'switchai.antigravity_surfaces_cache'

const DEFAULT_SURFACES: AntigravitySurface[] = [
  { id: 'antigravity', name: 'Antigravity', description: 'Desktop App', installed: true, running: false, path: null },
  { id: 'ide', name: 'Antigravity IDE', description: 'AI Code Editor', installed: true, running: false, path: null },
  { id: 'cli', name: 'Antigravity CLI', description: 'agy command-line', installed: true, running: false, path: null }
]

let globalCachedSurfaces: AntigravitySurface[] | null = null
let refreshInFlight: Promise<AntigravitySurface[]> | null = null
let lastRefreshAt = 0

function setCachedSurfaces(surfaces: AntigravitySurface[]): void {
  const previous = globalCachedSurfaces
  if (previous?.length === surfaces.length && previous.every((surface, index) => {
    const next = surfaces[index]
    return surface.id === next.id && surface.name === next.name &&
      surface.description === next.description && surface.running === next.running &&
      surface.installed === next.installed && surface.path === next.path
  })) return
  globalCachedSurfaces = surfaces
  try {
    localStorage.setItem(ANTIGRAVITY_SURFACES_CACHE_KEY, JSON.stringify(surfaces))
  } catch {
    // Preserve the in-memory snapshot when storage is unavailable.
  }
}

export function getInitialSurfaces(): AntigravitySurface[] {
  if (globalCachedSurfaces && globalCachedSurfaces.length > 0) {
    return globalCachedSurfaces
  }
  if (typeof window !== 'undefined') {
    try {
      const raw = localStorage.getItem(ANTIGRAVITY_SURFACES_CACHE_KEY)
      if (raw) {
        const parsed = JSON.parse(raw) as AntigravitySurface[]
        if (Array.isArray(parsed) && parsed.length > 0) {
          globalCachedSurfaces = parsed
          return parsed
        }
      }
    } catch {
      // ignore parse error
    }
  }
  return DEFAULT_SURFACES
}

export function refreshAntigravitySurfaces(): Promise<AntigravitySurface[]> {
  if (refreshInFlight) return refreshInFlight
  // Visibility and focus are commonly delivered together. Share a recently
  // completed scan as well as an in-flight one, including the startup warmup.
  if (globalCachedSurfaces && Date.now() - lastRefreshAt < 1000) {
    return Promise.resolve(globalCachedSurfaces)
  }
  refreshInFlight = api.getAntigravitySurfaces().then((data) => {
    if (data.length > 0) {
      setCachedSurfaces(data)
      lastRefreshAt = Date.now()
      return globalCachedSurfaces!
    }
    return data
  }).finally(() => {
    refreshInFlight = null
  })
  return refreshInFlight
}
