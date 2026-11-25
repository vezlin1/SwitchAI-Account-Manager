import { useEffect } from 'react'
import { isMac } from '../utils/platform'

export type ShortcutHandler = (event: KeyboardEvent) => void

export function useKeyboardShortcuts(
  shortcuts: Record<string, ShortcutHandler>,
  enabled = true
) {
  useEffect(() => {
    if (!enabled) return

    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const isInput =
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.isContentEditable)

      // Allow Escape even inside input elements
      if (isInput && event.key !== 'Escape') {
        return
      }

      const mod = isMac ? event.metaKey : event.ctrlKey
      const shift = event.shiftKey
      const alt = event.altKey
      const key = event.key.toLowerCase()

      let combo = ''
      if (mod) combo += 'mod+'
      if (shift) combo += 'shift+'
      if (alt) combo += 'alt+'
      combo += key

      const handler = shortcuts[combo] || shortcuts[key]
      if (handler) {
        event.preventDefault()
        handler(event)
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [shortcuts, enabled])
}
