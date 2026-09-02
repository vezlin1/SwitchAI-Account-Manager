import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react'

const PRIVACY_STORAGE_KEY = 'switchai:privacy-mode'

export type PrivacyContextType = {
  privacyMode: boolean
  setPrivacyMode: (enabled: boolean) => void
  togglePrivacyMode: () => void
  maskEmail: (email?: string | null) => string
  maskAccountId: (id?: string | null) => string
}

const PrivacyContext = createContext<PrivacyContextType | null>(null)

export function maskEmail(email?: string | null): string {
  if (!email) return '••••••'
  const atIndex = email.indexOf('@')
  if (atIndex <= 0) return '••••••••'
  const local = email.slice(0, atIndex)
  const domain = email.slice(atIndex)

  if (local.length <= 2) {
    return `${local[0]}•••${domain}`
  }
  if (local.length <= 4) {
    return `${local[0]}••••${local.slice(-1)}${domain}`
  }
  return `${local.slice(0, 2)}••••••${local.slice(-1)}${domain}`
}

export function maskAccountId(id?: string | null): string {
  if (!id) return '••••••••'
  if (id.length <= 6) return '••••••••'
  return `${id.slice(0, 3)}••••${id.slice(-3)}`
}

export function PrivacyProvider({ children }: { children: ReactNode }) {
  const [privacyMode, setPrivacyModeState] = useState<boolean>(() => {
    try {
      return localStorage.getItem(PRIVACY_STORAGE_KEY) === 'true'
    } catch {
      return false
    }
  })

  useEffect(() => {
    try {
      localStorage.setItem(PRIVACY_STORAGE_KEY, String(privacyMode))
    } catch {
      // ignore storage write errors
    }
  }, [privacyMode])

  const setPrivacyMode = useCallback((enabled: boolean) => {
    setPrivacyModeState(enabled)
  }, [])

  const togglePrivacyMode = useCallback(() => {
    setPrivacyModeState((prev) => !prev)
  }, [])

  return (
    <PrivacyContext.Provider
      value={{
        privacyMode,
        setPrivacyMode,
        togglePrivacyMode,
        maskEmail,
        maskAccountId
      }}
    >
      {children}
    </PrivacyContext.Provider>
  )
}

export function usePrivacy(): PrivacyContextType {
  const context = useContext(PrivacyContext)
  if (!context) {
    return {
      privacyMode: false,
      setPrivacyMode: () => undefined,
      togglePrivacyMode: () => undefined,
      maskEmail: (e) => e ?? '',
      maskAccountId: (id) => id ?? ''
    }
  }
  return context
}
