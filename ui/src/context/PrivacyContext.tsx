import { useCallback, useEffect, useState, type ReactNode } from 'react'
import { maskAccountId, maskEmail } from '../utils/privacy'
import { PrivacyContext } from './privacyContextDef'

const PRIVACY_STORAGE_KEY = 'switchai:privacy-mode'

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
