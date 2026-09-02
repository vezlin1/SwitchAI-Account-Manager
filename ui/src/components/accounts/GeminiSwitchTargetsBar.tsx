import { useEffect, useState } from 'react'
import { AppWindow, Code2, Info, Terminal } from 'lucide-react'
import { api } from '../../api'
import type { AntigravitySurface, AntigravitySurfaceId, AppSettings } from '../../types'

type GeminiSwitchTargetsBarProps = {
  appSettings: AppSettings
  onSaveAppSettings: (settings: AppSettings) => Promise<unknown>
}

const SURFACE_ICONS: Record<AntigravitySurfaceId, typeof AppWindow> = {
  antigravity: AppWindow,
  ide: Code2,
  cli: Terminal
}

const CACHE_KEY = 'switchai.antigravity_surfaces_cache'

const DEFAULT_SURFACES: AntigravitySurface[] = [
  { id: 'antigravity', name: 'Antigravity', description: 'Desktop App', installed: true, running: false, path: null },
  { id: 'ide', name: 'Antigravity IDE', description: 'AI Code Editor', installed: true, running: false, path: null },
  { id: 'cli', name: 'Antigravity CLI', description: 'agy command-line', installed: true, running: false, path: null }
]

let globalCachedSurfaces: AntigravitySurface[] | null = null

function getInitialSurfaces(): AntigravitySurface[] {
  if (globalCachedSurfaces && globalCachedSurfaces.length > 0) {
    return globalCachedSurfaces
  }
  if (typeof window !== 'undefined') {
    try {
      const raw = localStorage.getItem(CACHE_KEY)
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
          localStorage.setItem(CACHE_KEY, JSON.stringify(data))
        } catch {
          // ignore
        }
      }
    })
    .catch(() => undefined)
}

export function GeminiSwitchTargetsBar({
  appSettings,
  onSaveAppSettings
}: GeminiSwitchTargetsBarProps) {
  const [surfaces, setSurfaces] = useState<AntigravitySurface[]>(getInitialSurfaces)
  const [lastToggleWarning, setLastToggleWarning] = useState<string | null>(null)

  const activeTargets: string[] = appSettings.geminiSwitchTargets && appSettings.geminiSwitchTargets.length > 0
    ? appSettings.geminiSwitchTargets
    : ['antigravity']

  useEffect(() => {
    let cancelled = false

    const fetchSurfaces = async () => {
      try {
        const data = await api.getAntigravitySurfaces()
        if (cancelled || !data || data.length === 0) return

        globalCachedSurfaces = data
        try {
          localStorage.setItem(CACHE_KEY, JSON.stringify(data))
        } catch {
          // ignore storage error
        }

        setSurfaces((prev) => {
          // Avoid re-render if data is identical
          if (
            prev.length === data.length &&
            prev.every((s, i) =>
              s.id === data[i].id &&
              s.running === data[i].running &&
              s.installed === data[i].installed &&
              s.name === data[i].name
            )
          ) {
            return prev
          }
          return data
        })
      } catch {
        // preserve existing cache on error
      }
    }

    // Immediate background fetch
    void fetchSurfaces()

    // Smooth background polling every 5s while mounted
    const interval = setInterval(() => {
      void fetchSurfaces()
    }, 5000)

    // Immediate refresh on window focus / tab visibility
    const handleFocus = () => {
      void fetchSurfaces()
    }
    window.addEventListener('focus', handleFocus)
    document.addEventListener('visibilitychange', handleFocus)

    return () => {
      cancelled = true
      clearInterval(interval)
      window.removeEventListener('focus', handleFocus)
      document.removeEventListener('visibilitychange', handleFocus)
    }
  }, [])

  const handleToggle = (id: AntigravitySurfaceId) => {
    setLastToggleWarning(null)
    const isCurrentlyActive = activeTargets.includes(id)

    if (isCurrentlyActive && activeTargets.length <= 1) {
      setLastToggleWarning('At least one target must remain active')
      setTimeout(() => setLastToggleWarning(null), 3000)
      return
    }

    const nextTargets = isCurrentlyActive
      ? activeTargets.filter((t) => t !== id)
      : [...activeTargets, id]

    void onSaveAppSettings({
      ...appSettings,
      geminiSwitchTargets: nextTargets
    })
  }

  const surfaceItems: AntigravitySurface[] = surfaces.length > 0
    ? surfaces
    : getInitialSurfaces()

  return (
    <div className="gemini-targets-bar flex items-center justify-between gap-3 px-3.5 py-2 rounded-xl bg-ag-surface/60 border border-ag-border/70 flex-wrap text-xs select-none">
      <div className="flex items-center gap-2">
        <span className="text-[11px] font-medium text-ag-muted tracking-wide flex items-center gap-1.5">
          <span>Switch account in:</span>
        </span>
        {lastToggleWarning && (
          <span className="text-[11px] text-amber-400 font-medium flex items-center gap-1 animate-fadeIn">
            <Info size={12} />
            {lastToggleWarning}
          </span>
        )}
      </div>

      <div className="flex items-center gap-1.5 flex-wrap" role="group" aria-label="Antigravity switch targets">
        {surfaceItems.map((surface) => {
          const Icon = SURFACE_ICONS[surface.id] ?? AppWindow
          const isActive = activeTargets.includes(surface.id)
          const isOnlyActive = isActive && activeTargets.length <= 1

          return (
            <button
              key={surface.id}
              type="button"
              onClick={() => handleToggle(surface.id)}
              className={`gemini-target-pill inline-flex items-center gap-2 px-2.5 py-1 rounded-lg text-xs font-medium transition-all cursor-pointer border select-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-400 ${
                isActive
                  ? 'bg-blue-500/15 border-blue-500/40 text-blue-300 shadow-sm hover:bg-blue-500/25 hover:border-blue-500/60 hover:text-blue-200 active:scale-95'
                  : 'bg-ag-card/40 border-ag-border/60 text-ag-muted hover:text-ag-text hover:border-ag-border hover:bg-ag-card active:scale-95'
              }`}
              title={`${surface.name} (${surface.description})${surface.running ? ' · Currently running' : surface.installed ? ' · Detected' : ''}${isOnlyActive ? ' · At least one target must remain active' : ''}`}
              aria-pressed={isActive}
            >
              <span className={`w-2 h-2 rounded-full transition-colors ${
                isActive
                  ? surface.running
                    ? 'bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.8)]'
                    : 'bg-blue-400'
                  : 'bg-zinc-600'
              }`} />
              <Icon size={13} className={isActive ? 'text-blue-400' : 'text-ag-muted'} aria-hidden="true" />
              <span>{surface.name}</span>
              {surface.running && (
                <span className="text-[10px] uppercase font-semibold tracking-wider text-emerald-400/90 bg-emerald-500/10 px-1 py-0.5 rounded">
                  running
                </span>
              )}
            </button>
          )
        })}
      </div>
    </div>
  )
}
