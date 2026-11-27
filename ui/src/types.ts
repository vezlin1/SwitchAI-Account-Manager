export type QuotaWindow = {
  usedPercent: number | null
  limitWindowSeconds: number | null
  resetAt: number | null
  fetchedAt: number | null
}

export type QuotaWindows = {
  primary: QuotaWindow
  secondary: QuotaWindow
}

export type QuotaInfo = QuotaWindows & {
  planType: string | null
  claudeAndGpt?: QuotaWindows
  fetchedAt: number
}

export type TokenHealthStatus =
  | 'unknown'
  | 'healthy'
  | 'refreshed'
  | 'needs_relogin'
  | 'network_error'
  | 'server_error'

export type TokenHealth = {
  status: TokenHealthStatus
  lastCheckedAt: number | null
  lastRefreshedAt: number | null
  lastError: string | null
}

export type AccountProvider = 'codex' | 'gemini'

export type AccountIssues = {
  quota: string | null
  subscription: string | null
}

export type Account = {
  id: string
  provider?: AccountProvider
  email: string | null
  accountId: string | null
  subscriptionExpiresAt: number | null
  subscriptionPlan: string | null
  subscriptionDetectedAt: number | null
  subscriptionCheckedAt: number | null
  tokenHealth: TokenHealth
  quota: QuotaInfo | null
  createdAt: number
  lastLoginAt: number
  issues: AccountIssues
}

export type AccountSnapshot = {
  revision: number
  account: Account
}

export type AccountTag = {
  id: `tag:${string}`
  name: string
  provider: AccountProvider
  accountIds: string[]
  showInAll: boolean
  refreshOnlyInTab: boolean
}

export type AppSettings = {
  autoRefreshEnabled: boolean
  autoRefreshIntervalMinutes: number
  closeToTray: boolean
  skipUnsupportedRegionRefresh: boolean
  hiddenSubscriptionCategories: string[]
  hiddenAccountIds: string[]
  accountTags: AccountTag[]
  lastActiveProvider?: AccountProvider
  geminiSwitchTargets?: string[]
  enabledProviders?: AccountProvider[]
  privacyMode?: boolean
  privacyMigrationPending?: boolean
  geminiQuotaFocus?: GeminiQuotaFocus
}

export type GeminiQuotaFocus = 'gemini' | 'claude_and_gpt' | 'both'

export type RecommendationReason = 'most_remaining' | 'resets_soon' | 'larger_plan'

export type Recommendation = {
  accountId: string
  remainingPercent: number
  projected: boolean
  reason: RecommendationReason
}

export type AppData = {
  revision: number
  accounts: Account[]
  activeAccountId: string | null
  activeGeminiAccountId?: string | null
  appSettings: AppSettings
}

export type CommandWarning = {
  code: string
  domain: string
  message: string
  accountId: string | null
  retryable: boolean
}

export type StateResult = {
  state: AppData
  warnings: CommandWarning[]
}

export type AccountRefreshResult = {
  account: Account
  warnings: CommandWarning[]
}

export type SwitchAccountRestartResponse = {
  state: AppData
  restartWarning: string | null
  clientNotice: string | null
}

export type IpcError = {
  code: string
  domain: string
  message: string
  accountId: string | null
  retryable: boolean
}

export type RecoveryStatus = {
  error: string
  dataDirectory: string
  statePath: string
  backupAvailable: boolean
}

export type StartupStatus = {
  mode: string
  state: AppData | null
  warnings: string[]
  recovery: RecoveryStatus | null
  autoRefresh: AutoRefreshStatus | null
}

export type AppStateChangedEvent = {
  scope: 'account' | 'accounts' | 'settings' | string
  accountIds: string[]
  revision: number
}

export type AntigravitySurfaceId = 'antigravity' | 'ide' | 'cli'

export type AntigravitySurface = {
  id: AntigravitySurfaceId
  name: string
  description: string
  installed: boolean
  running: boolean
  path: string | null
}

export type RefreshRunSummary = {
  startedAt: number
  finishedAt: number
  succeeded: number
  failed: number
  failedAccountIds: string[]
  warnings: string[]
}

export type AutoRefreshStatus = {
  enabled: boolean
  inFlight: boolean
  refreshingAccountIds: string[]
  progress: { provider: AccountProvider; total: number; completed: number; failed: number }[]
  lastStartedAt: number | null
  lastFinishedAt: number | null
  lastError: string | null
  nextRunAt: number | null
  scheduledAccounts: number
  backedOffAccounts: number
  lastRun: RefreshRunSummary | null
}

export type OAuthStartResponse = {
  flowId: string
  authorizationUrl: string
}

export type OAuthFlowResponse = {
  flowId: string
  authorizationUrl: string
  callbackUrl: string | null
  createdAt: number
  status: 'waiting_callback' | 'exchanging' | 'completed' | 'error' | 'cancelled'
  error: string | null
  account: Account | null
}
