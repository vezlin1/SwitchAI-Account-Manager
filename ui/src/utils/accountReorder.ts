import type { Account } from '../types'

/**
 * Reorders visible accounts when a user drags `activeId` to `overId` in a filtered or
 * provider-specific view, preserving the exact slot positions of all non-visible accounts
 * (such as accounts belonging to another provider or excluded by subscription filters).
 */
export function reorderFilteredAccounts(
  allAccounts: Account[],
  visibleAccounts: Account[],
  activeId: string,
  overId: string
): Account[] {
  if (activeId === overId) return allAccounts

  const visibleOldIndex = visibleAccounts.findIndex((account) => account.id === activeId)
  const visibleNewIndex = visibleAccounts.findIndex((account) => account.id === overId)
  if (visibleOldIndex < 0 || visibleNewIndex < 0 || visibleOldIndex === visibleNewIndex) {
    return allAccounts
  }

  const reorderedVisible = [...visibleAccounts]
  const [moved] = reorderedVisible.splice(visibleOldIndex, 1)
  reorderedVisible.splice(visibleNewIndex, 0, moved)

  const visibleIds = new Set(visibleAccounts.map((account) => account.id))

  let visibleCursor = 0
  return allAccounts.map((account) => {
    if (visibleIds.has(account.id)) {
      const replacement = reorderedVisible[visibleCursor]
      visibleCursor += 1
      return replacement
    }
    return account
  })
}
