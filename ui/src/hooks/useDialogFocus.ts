import { useEffect, useRef, type RefObject } from 'react'

const FOCUSABLE = [
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  'a[href]',
  '[tabindex]:not([tabindex="-1"])'
].join(',')

export function useDialogFocus(
  dialogRef: RefObject<HTMLElement | null>,
  onClose: () => void,
  escapeEnabled = true
) {
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  }, [onClose])

  useEffect(() => {
    const previouslyFocused = document.activeElement as HTMLElement | null
    const dialog = dialogRef.current
    if (!dialog) return
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'

    const initial = dialog.querySelector<HTMLElement>('[data-autofocus]')
      ?? dialog.querySelector<HTMLElement>(FOCUSABLE)
      ?? dialog
    const frame = window.requestAnimationFrame(() => initial.focus())

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && escapeEnabled) {
        event.preventDefault()
        onCloseRef.current()
        return
      }
      if (event.key !== 'Tab') return

      const focusable = [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE)]
        .filter((element) => !element.hidden && element.getClientRects().length > 0)
      if (!focusable.length) {
        event.preventDefault()
        dialog.focus()
        return
      }

      const first = focusable[0]
      const last = focusable.at(-1) ?? first
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => {
      window.cancelAnimationFrame(frame)
      document.removeEventListener('keydown', handleKeyDown)
      document.body.style.overflow = previousOverflow
      previouslyFocused?.focus()
    }
  }, [dialogRef, escapeEnabled])
}
