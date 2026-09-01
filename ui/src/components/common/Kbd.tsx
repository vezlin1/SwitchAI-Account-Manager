import React from 'react'
import { usePlatform } from '../../hooks/usePlatform'

interface KbdProps {
  children?: React.ReactNode
  combo?: string
  className?: string
}

export function Kbd({ children, combo, className = '' }: KbdProps) {
  const { isMac } = usePlatform()

  let text = children ? String(children) : (combo ?? '')

  if (isMac) {
    text = text
      .replace(/ctrl\+/gi, '⌃')
      .replace(/cmd\+/gi, '⌘')
      .replace(/mod\+/gi, '⌘')
      .replace(/meta\+/gi, '⌘')
      .replace(/alt\+/gi, '⌥')
      .replace(/shift\+/gi, '⇧')
      .replace(/enter/gi, '↵')
      .replace(/return/gi, '↵')
      .replace(/backspace/gi, '⌫')
      .replace(/delete/gi, '⌦')
      .replace(/esc/gi, '⎋')
    text = text.replace(/([⌘⌃⌥⇧])([a-z])$/, (_, mod: string, key: string) => `${mod}${key.toUpperCase()}`)
  } else {
    text = text
      .replace(/mod\+/gi, 'Ctrl+')
      .replace(/cmd\+/gi, 'Ctrl+')
      .replace(/meta\+/gi, 'Win+')
  }

  return (
    <kbd className={`kbd-badge ${className}`}>
      {text}
    </kbd>
  )
}
