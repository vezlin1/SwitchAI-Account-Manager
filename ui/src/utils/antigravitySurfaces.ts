import { api } from '../api'
import type { AntigravitySurface } from '../types'

export const ANTIGRAVITY_SURFACES_CACHE_KEY = 'switchai.antigravity_surfaces_cache'

export const DEFAULT_SURFACES: AntigravitySurface[] = [
  { id: 'antigravity', name: 'Antigravity', description: 'Desktop App', installed: true, running: false, path: null },
  { id: 'ide', name: 'Antigravity IDE', description: 'AI Code Editor', installed: true, running: false, path: null },
  { id: 'cli', name: 'Antigravity CLI', description: 'agy command-line', installed: true, running: false, path: null }
]

let globalCachedSurfaces: AntigravitySurface[] | null = null

export function getCachedSurfaces(): AntigravitySurface[] | null {
  return globalCachedSurfaces
}

export function setCachedSurfaces(surfaces: AntigravitySurface[]): void {
  globalCachedSurfaces = surfaces
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

export function warmUpAntigravitySurfacesCache(): void {
  void api.getAntigravitySurfaces()
    .then((data) => {
      if (data && data.length > 0) {
        globalCachedSurfaces = data
        try {
          localStorage.setItem(ANTIGRAVITY_SURFACES_CACHE_KEY, JSON.stringify(data))
        } catch {
          // ignore
        }
      }
    })
    .catch(() => undefined)
}
