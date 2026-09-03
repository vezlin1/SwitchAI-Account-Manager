import { useContext } from 'react'
import { PrivacyContext, type PrivacyContextType } from './privacyContextDef'

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
