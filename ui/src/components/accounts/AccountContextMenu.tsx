import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Check, Copy, Eye, EyeOff } from 'lucide-react'
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
const MENU_HEIGHT = 120
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
  const [copiedKey, setCopiedKey] = useState<'email' | 'id' | null>(null)
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

  const copyText = async (key: 'email' | 'id', text?: string | null) => {
    if (!text) return
    try {
      await navigator.clipboard.writeText(text)
      setCopiedKey(key)
      setTimeout(() => {
        onClose()
        if (keyboardTriggered) {
          window.requestAnimationFrame(() => opener.focus())
        }
      }, 500)
    } catch {
      onClose()
    }
  }

  return createPortal(
    <div
      ref={menuRef}
      className="account-context-menu"
      role="menu"
      aria-label={`Actions for ${account.email ?? 'account'}`}
      style={{ left, top }}
      onContextMenu={(event) => event.preventDefault()}
    >
      {account.email && (
        <button
          type="button"
          role="menuitem"
          className="account-context-menu-item"
          onClick={() => void copyText('email', account.email)}
        >
          <div className="flex items-center gap-2">
            {copiedKey === 'email' ? <Check size={14} className="text-green-400" /> : <Copy size={14} />}
            <span>{copiedKey === 'email' ? 'Email copied!' : 'Copy email'}</span>
          </div>
        </button>
      )}

      {account.accountId && (
        <button
          type="button"
          role="menuitem"
          className="account-context-menu-item"
          onClick={() => void copyText('id', account.accountId)}
        >
          <div className="flex items-center gap-2">
            {copiedKey === 'id' ? <Check size={14} className="text-green-400" /> : <Copy size={14} />}
            <span>{copiedKey === 'id' ? 'ID copied!' : 'Copy account ID'}</span>
          </div>
        </button>
      )}

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
