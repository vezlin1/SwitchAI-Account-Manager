import { useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { Eye, EyeOff } from 'lucide-react'
import { Kbd } from '../common'
import type { Account } from '../../types'

type AccountContextMenuProps = {
  account: Account
  hiddenFromAll: boolean
  x: number
  y: number
  opener: HTMLElement
  keyboardTriggered: boolean
  onToggleInAll: (accountId: string) => void
  onClose: () => void
}

const MENU_WIDTH = 224
const MENU_HEIGHT = 56
const VIEWPORT_GAP = 8

export function AccountContextMenu({
  account,
  hiddenFromAll,
  x,
  y,
  opener,
  keyboardTriggered,
  onToggleInAll,
  onClose
}: AccountContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null)
  const left = Math.max(VIEWPORT_GAP, Math.min(x, window.innerWidth - MENU_WIDTH - VIEWPORT_GAP))
  const top = Math.max(VIEWPORT_GAP, Math.min(y, window.innerHeight - MENU_HEIGHT - VIEWPORT_GAP))

  useEffect(() => {
    const menuEl = menuRef.current
    menuEl?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus()

    const closeForPointer = (event: PointerEvent) => {
      if (!menuEl?.contains(event.target as Node)) onClose()
    }
    const closeForKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
        window.requestAnimationFrame(() => opener.focus())
      } else if (event.key === 'h' || event.key === 'H') {
        event.preventDefault()
        onToggleInAll(account.id)
        onClose()
        window.requestAnimationFrame(() => opener.focus())
      }
    }
    const handleFocusOut = (event: FocusEvent) => {
      if (!menuEl?.contains(event.relatedTarget as Node)) {
        onClose()
      }
    }
    const closeForViewportChange = () => onClose()

    document.addEventListener('pointerdown', closeForPointer)
    document.addEventListener('keydown', closeForKey)
    menuEl?.addEventListener('focusout', handleFocusOut)
    window.addEventListener('resize', closeForViewportChange)
    window.addEventListener('scroll', closeForViewportChange, true)
    return () => {
      document.removeEventListener('pointerdown', closeForPointer)
      document.removeEventListener('keydown', closeForKey)
      menuEl?.removeEventListener('focusout', handleFocusOut)
      window.removeEventListener('resize', closeForViewportChange)
      window.removeEventListener('scroll', closeForViewportChange, true)
    }
  }, [onClose, opener, account.id, onToggleInAll])

  return createPortal(
    <div
      ref={menuRef}
      className="account-context-menu"
      role="menu"
      aria-label={`Actions for ${account.email ?? 'account'}`}
      style={{ left, top }}
      onContextMenu={(event) => event.preventDefault()}
    >
      <button
        type="button"
        role="menuitem"
        className="account-context-menu-item"
        onClick={() => {
          onToggleInAll(account.id)
          onClose()
          if (keyboardTriggered) {
            window.requestAnimationFrame(() => opener.focus())
          }
        }}
      >
        <div className="flex items-center gap-2">
          {hiddenFromAll ? (
            <Eye size={14} aria-hidden="true" />
          ) : (
            <EyeOff size={14} aria-hidden="true" />
          )}
          <span>{hiddenFromAll ? 'Show in All' : 'Hide from All'}</span>
        </div>
        <Kbd combo="H" />
      </button>
    </div>,
    document.body
  )
}
