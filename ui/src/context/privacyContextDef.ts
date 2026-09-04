import { createContext } from 'react'

export type PrivacyContextType = {
  privacyMode: boolean
  setPrivacyMode: (enabled: boolean) => void
  togglePrivacyMode: () => void
  maskEmail: (email?: string | null) => string
  maskAccountId: (id?: string | null) => string
}

export const PrivacyContext = createContext<PrivacyContextType | null>(null)

