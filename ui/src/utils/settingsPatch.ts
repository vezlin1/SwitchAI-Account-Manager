import type { AppSettings } from '../types'

export type SettingsPatch = Partial<AppSettings>

function sameStrings(left: readonly string[] | undefined, right: readonly string[] | undefined): boolean {
  return left === right || (
    left != null && right != null &&
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  )
}

export function settingsPatch(previous: AppSettings, next: AppSettings): SettingsPatch {
  const patch: SettingsPatch = {}
  if (previous.autoRefreshEnabled !== next.autoRefreshEnabled) patch.autoRefreshEnabled = next.autoRefreshEnabled
  if (previous.autoRefreshIntervalMinutes !== next.autoRefreshIntervalMinutes) patch.autoRefreshIntervalMinutes = next.autoRefreshIntervalMinutes
  if (previous.closeToTray !== next.closeToTray) patch.closeToTray = next.closeToTray
  if (previous.skipUnsupportedRegionRefresh !== next.skipUnsupportedRegionRefresh) {
    patch.skipUnsupportedRegionRefresh = next.skipUnsupportedRegionRefresh
  }
  if (!sameStrings(previous.hiddenSubscriptionCategories, next.hiddenSubscriptionCategories)) {
    patch.hiddenSubscriptionCategories = next.hiddenSubscriptionCategories
  }
  if (!sameStrings(previous.hiddenAccountIds, next.hiddenAccountIds)) {
    patch.hiddenAccountIds = next.hiddenAccountIds
  }
  if (previous.lastActiveProvider !== next.lastActiveProvider) {
    patch.lastActiveProvider = next.lastActiveProvider
  }
  if (!sameStrings(previous.geminiSwitchTargets, next.geminiSwitchTargets)) {
    patch.geminiSwitchTargets = next.geminiSwitchTargets
  }
  if (!sameStrings(previous.enabledProviders, next.enabledProviders)) {
    patch.enabledProviders = next.enabledProviders
  }
  if (previous.privacyMode !== next.privacyMode && next.privacyMode !== undefined) {
    patch.privacyMode = next.privacyMode
  }
  return patch
}
