import { useState } from 'react'

type SwitchProps = {
  id?: string
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  ariaLabel?: string
}

export function Switch({
  id,
  checked,
  onChange,
  disabled = false,
  ariaLabel
}: SwitchProps) {
  const [isPressed, setIsPressed] = useState(false)

  const toggle = () => {
    if (!disabled) {
      onChange(!checked)
    }
  }

  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation()
        toggle()
      }}
      onMouseDown={(e) => {
        e.stopPropagation()
        setIsPressed(true)
      }}
      onMouseUp={(e) => {
        e.stopPropagation()
        setIsPressed(false)
      }}
      onMouseLeave={() => setIsPressed(false)}
      onKeyDown={(e) => {
        if (e.key === ' ' || e.key === 'Enter') {
          e.preventDefault()
          e.stopPropagation()
          setIsPressed(true)
        }
      }}
      onKeyUp={(e) => {
        if (e.key === ' ' || e.key === 'Enter') {
          e.preventDefault()
          e.stopPropagation()
          setIsPressed(false)
          toggle()
        }
      }}
      className={`ag-switch ${checked ? 'ag-switch-checked' : ''} ${isPressed ? 'ag-switch-pressed' : ''}`}
    >
      <span className="ag-switch-thumb" />
    </button>
  )
}
