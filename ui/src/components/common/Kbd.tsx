import { isMac } from '../../utils/platform'

interface KbdProps {
  combo: string
}

export function Kbd({ combo }: KbdProps) {
  let text = combo

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
    <kbd className="kbd-badge">
      {text}
    </kbd>
  )
}
