import type { AppSettings } from '../types'

export type SettingsPatch = Partial<AppSettings>

export function settingsPatch(previous: AppSettings, next: AppSettings): SettingsPatch {
  const patch: SettingsPatch = {}
  if (previous.autoCheckUpdates !== next.autoCheckUpdates) patch.autoCheckUpdates = next.autoCheckUpdates
  if (previous.autoRefreshEnabled !== next.autoRefreshEnabled) patch.autoRefreshEnabled = next.autoRefreshEnabled
  if (previous.autoRefreshIntervalMinutes !== next.autoRefreshIntervalMinutes) patch.autoRefreshIntervalMinutes = next.autoRefreshIntervalMinutes
  if (previous.closeToTray !== next.closeToTray) patch.closeToTray = next.closeToTray
  if (previous.skipUnsupportedRegionRefresh !== next.skipUnsupportedRegionRefresh) {
    patch.skipUnsupportedRegionRefresh = next.skipUnsupportedRegionRefresh
  }
  if (JSON.stringify(previous.hiddenSubscriptionCategories) !== JSON.stringify(next.hiddenSubscriptionCategories)) {
    patch.hiddenSubscriptionCategories = next.hiddenSubscriptionCategories
  }
  if (JSON.stringify(previous.hiddenAccountIds) !== JSON.stringify(next.hiddenAccountIds)) {
    patch.hiddenAccountIds = next.hiddenAccountIds
  }
  if (previous.lastActiveProvider !== next.lastActiveProvider) {
    patch.lastActiveProvider = next.lastActiveProvider
  }
  if (JSON.stringify(previous.geminiSwitchTargets) !== JSON.stringify(next.geminiSwitchTargets)) {
    patch.geminiSwitchTargets = next.geminiSwitchTargets
  }
  if (JSON.stringify(previous.enabledProviders) !== JSON.stringify(next.enabledProviders)) {
    patch.enabledProviders = next.enabledProviders
  }
  if (previous.privacyMode !== next.privacyMode && next.privacyMode !== undefined) {
    patch.privacyMode = next.privacyMode
  }
  return patch
}
