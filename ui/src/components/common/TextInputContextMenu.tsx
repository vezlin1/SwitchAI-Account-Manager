import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { CheckSquare, ClipboardPaste, Copy, Scissors, Trash2 } from 'lucide-react'
import { Kbd } from './Kbd'

interface MenuPosition {
  x: number
  y: number
  target: HTMLInputElement | HTMLTextAreaElement
}

const MENU_WIDTH = 180
const MENU_HEIGHT = 160
const VIEWPORT_GAP = 8

function setNativeInputValue(element: HTMLInputElement | HTMLTextAreaElement, nextValue: string) {
  const isTextarea = element.tagName === 'TEXTAREA'
  const proto = isTextarea ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype
  const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set

  if (setter) {
    setter.call(element, nextValue)
  } else {
    element.value = nextValue
  }

  element.dispatchEvent(new Event('input', { bubbles: true }))
  element.dispatchEvent(new Event('change', { bubbles: true }))
}

export function TextInputContextMenu() {
  const [menu, setMenu] = useState<MenuPosition | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handleGlobalContextMenu = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null
      const isInput = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA'

      if (isInput && target instanceof HTMLInputElement || (isInput && target instanceof HTMLTextAreaElement)) {
        // Only for text/search/password/number inputs
        const type = (target as HTMLInputElement).type
        if (target instanceof HTMLInputElement && ['checkbox', 'radio', 'button', 'submit', 'file'].includes(type)) {
          event.preventDefault()
          setMenu(null)
          return
        }

        event.preventDefault()
        event.stopPropagation()

        setMenu({
          x: event.clientX,
          y: event.clientY,
          target: target as HTMLInputElement | HTMLTextAreaElement
        })
      } else {
        event.preventDefault()
        setMenu(null)
      }
    }

    window.addEventListener('contextmenu', handleGlobalContextMenu, true)
    return () => {
      window.removeEventListener('contextmenu', handleGlobalContextMenu, true)
    }
  }, [])

  useEffect(() => {
    if (!menu) return

    const handlePointerDown = (event: PointerEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenu(null)
      }
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setMenu(null)
        menu.target.focus()
      }
    }

    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    window.addEventListener('resize', () => setMenu(null))
    window.addEventListener('scroll', () => setMenu(null), true)

    return () => {
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [menu])

  if (!menu) return null

  const target = menu.target
  const hasText = Boolean(target.value && target.value.length > 0)
  const selStart = target.selectionStart ?? 0
  const selEnd = target.selectionEnd ?? 0
  const hasSelection = selStart !== selEnd
  const selectedText = hasSelection ? target.value.slice(selStart, selEnd) : ''

  const left = Math.max(VIEWPORT_GAP, Math.min(menu.x, window.innerWidth - MENU_WIDTH - VIEWPORT_GAP))
  const top = Math.max(VIEWPORT_GAP, Math.min(menu.y, window.innerHeight - MENU_HEIGHT - VIEWPORT_GAP))

  const handleCut = async () => {
    if (!hasSelection) return
    try {
      await navigator.clipboard.writeText(selectedText)
      const nextValue = target.value.slice(0, selStart) + target.value.slice(selEnd)
      setNativeInputValue(target, nextValue)
      target.setSelectionRange(selStart, selStart)
      target.focus()
    } catch {
      /* ignore clipboard permission error */
    }
    setMenu(null)
  }

  const handleCopy = async () => {
    if (!hasSelection) return
    try {
      await navigator.clipboard.writeText(selectedText)
      target.focus()
    } catch {
      /* ignore clipboard permission error */
    }
    setMenu(null)
  }

  const handlePaste = async () => {
    try {
      const text = await navigator.clipboard.readText()
      if (text) {
        const nextValue = target.value.slice(0, selStart) + text + target.value.slice(selEnd)
        const nextCursor = selStart + text.length
        setNativeInputValue(target, nextValue)
        target.setSelectionRange(nextCursor, nextCursor)
        target.focus()
      }
    } catch {
      /* ignore clipboard permission error */
    }
    setMenu(null)
  }

  const handleSelectAll = () => {
    target.select()
    target.focus()
    setMenu(null)
  }

  const handleClear = () => {
    setNativeInputValue(target, '')
    target.focus()
    setMenu(null)
  }

  return createPortal(
    <div
      ref={menuRef}
      className="account-context-menu"
      style={{ left, top, minWidth: '170px' }}
      role="menu"
      aria-label="Edit menu"
    >
      <button
        type="button"
        role="menuitem"
        className="account-context-menu-item"
        onClick={handleCut}
        disabled={!hasSelection}
      >
        <div className="flex items-center gap-2">
          <Scissors size={13} aria-hidden="true" />
          <span>Cut</span>
        </div>
        <Kbd combo="mod+X" />
      </button>

      <button
        type="button"
        role="menuitem"
        className="account-context-menu-item"
        onClick={handleCopy}
        disabled={!hasSelection}
      >
        <div className="flex items-center gap-2">
          <Copy size={13} aria-hidden="true" />
          <span>Copy</span>
        </div>
        <Kbd combo="mod+C" />
      </button>

      <button
        type="button"
        role="menuitem"
        className="account-context-menu-item"
        onClick={handlePaste}
      >
        <div className="flex items-center gap-2">
          <ClipboardPaste size={13} aria-hidden="true" />
          <span>Paste</span>
        </div>
        <Kbd combo="mod+V" />
      </button>

      <div className="account-context-menu-divider" />

      <button
        type="button"
        role="menuitem"
        className="account-context-menu-item"
        onClick={handleSelectAll}
        disabled={!hasText}
      >
        <div className="flex items-center gap-2">
          <CheckSquare size={13} aria-hidden="true" />
          <span>Select all</span>
        </div>
        <Kbd combo="mod+A" />
      </button>

      {hasText && (
        <button
          type="button"
          role="menuitem"
          className="account-context-menu-item account-context-menu-item-danger"
          onClick={handleClear}
        >
          <div className="flex items-center gap-2">
            <Trash2 size={13} aria-hidden="true" />
            <span>Clear</span>
          </div>
        </button>
      )}
    </div>,
    document.body
  )
}
