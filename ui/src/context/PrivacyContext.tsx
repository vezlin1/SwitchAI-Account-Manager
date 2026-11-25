import { useEffect, useState, type ReactNode } from 'react'
import { maskAccountId, maskEmail } from '../utils/privacy'
import { PrivacyContext } from './privacyContextDef'

const PRIVACY_STORAGE_KEY = 'switchai:privacy-mode'

export function PrivacyProvider({ children }: { children: ReactNode }) {
  const [privacyMode, setPrivacyMode] = useState<boolean>(() => {
    if (typeof window === 'undefined') return false
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
      // Ignore localStorage errors in restricted environments
    }
  }, [privacyMode])

  return (
    <PrivacyContext.Provider
      value={{
        privacyMode,
        setPrivacyMode,
        maskEmail,
        maskAccountId
      }}
    >
      {children}
    </PrivacyContext.Provider>
  )
}
