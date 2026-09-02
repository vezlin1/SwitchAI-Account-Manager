import { invoke } from '@tauri-apps/api/core'
import type {
  AccountProvider,
  AccountSnapshot,
  AntigravitySurface,
  AppData,
  AppSettings,
  AutoRefreshStatus,
  IpcError,
  OAuthFlowResponse,
  OAuthStartResponse,
  StartupStatus,
  StateResult,
  SwitchAccountRestartResponse
} from './types'

function toIpcError(error: unknown): IpcError {
  if (typeof error === 'string') {
    const trimmed = error.trim()
    if (trimmed.startsWith('{')) {
      try {
        return toIpcError(JSON.parse(trimmed))
      } catch {
        // Fall through to the plain IPC string.
      }
    }
    return {
      code: 'ipc_unknown',
      domain: 'system',
      message: trimmed || 'Unknown IPC error',
      accountId: null,
      retryable: false
    }
  }

  if (error && typeof error === 'object') {
    const candidate = error as Partial<IpcError>
    if (typeof candidate.message === 'string' || typeof candidate.code === 'string') {
      return {
        code: candidate.code ?? 'ipc_unknown',
        domain: candidate.domain ?? 'system',
        message: candidate.message ?? String(error),
        accountId: candidate.accountId ?? null,
        retryable: Boolean(candidate.retryable)
      }
    }
  }

  return {
    code: 'ipc_unknown',
    domain: 'system',
    message: String(error),
    accountId: null,
    retryable: false
  }
}

export function describeIpcError(error: unknown): string {
  return toIpcError(error).message || String(error)
}

export const api = {
  getState: () => invoke<AppData>('get_app_state'),
  getStartupStatus: () => invoke<StartupStatus>('get_startup_status'),
  restoreStateBackup: () => invoke<StartupStatus>('restore_state_backup'),
  startFresh: () => invoke<StartupStatus>('start_fresh'),
  openRecoveryDataDirectory: () => invoke<void>('open_recovery_data_directory'),
  getAccount: (accountId: string) =>
    invoke<AccountSnapshot>('get_account', { accountId }),
  getAutoRefreshStatus: () =>
    invoke<AutoRefreshStatus>('get_auto_refresh_status'),
  setAppSettings: (settings: AppSettings) =>
    invoke<AppData>('set_app_settings', { settings }),

  startOAuthFlow: (targetAccountId: string | null = null, provider: string | null = null) =>
    invoke<OAuthStartResponse>('start_oauth_flow', { targetAccountId, provider }),
  getOAuthStatus: (flowId: string) =>
    invoke<OAuthFlowResponse>('get_oauth_flow_status', { flowId }),
  cancelOAuthFlow: (flowId: string) =>
    invoke<void>('cancel_oauth_flow', { flowId }),
  openExternalUrl: (url: string) =>
    invoke<void>('open_external_url', { url }),

  removeAccount: (accountId: string) =>
    invoke<AppData>('remove_account', { accountId }),
  switchActiveAccountAndRestartCodex: (accountId: string) =>
    invoke<SwitchAccountRestartResponse>(
      'switch_active_account_and_restart_codex',
      { accountId }
    ),
  switchActiveAccountAndRestartAntigravity: (accountId: string) =>
    invoke<SwitchAccountRestartResponse>(
      'switch_active_gemini_account_and_restart_antigravity',
      { accountId }
    ),
  getAntigravitySurfaces: () =>
    invoke<AntigravitySurface[]>('get_antigravity_surfaces'),
  importAntigravityAccount: () => invoke<StateResult>('import_antigravity_account'),
  importCodexAccount: () => invoke<StateResult>('import_codex_account'),
  setAccountOrder: (accountIds: string[]) =>
    invoke<AppData>('set_account_order', { accountIds }),
  refreshAccountSubscription: (accountId: string) =>
    invoke<StateResult>('refresh_account_subscription', { accountId }),
  refreshAccountQuota: (accountId: string) =>
    invoke<StateResult>('refresh_account_quota', { accountId }),
  refreshAllQuotas: (provider?: AccountProvider) =>
    invoke<StateResult>('refresh_all_quotas', { provider }),
}
