import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  AlertTriangle,
  Check,
  Clock,
  ExternalLink,
  Eye,
  EyeOff,
  Loader2,
  RefreshCw,
  X,
  Zap,
  Sparkles,
  ShieldCheck
} from 'lucide-react'
import appLogo from '../../assets/app-icon.png'
import { api, describeIpcError } from '../../api'
import { useAppData } from '../../hooks/useAppData'
import { usePrivacy } from '../../context/usePrivacy'
import { useSharedTicker } from '../../hooks/useSharedTicker'
import { accountRemainingPercent, recommendedAccount } from '../../utils/accountInsights'
import { formatTimeUntil } from '../../utils/format'
import { formatSubscriptionPlan } from '../../utils/dateUtils'
import { maskAccountId, maskEmail } from '../../utils/privacy'
import type { Account, AccountProvider } from '../../types'

function getAccountDisplayName(account: Account, privacyMode: boolean): string {
  if (privacyMode) {
    if (account.email?.trim()) return maskEmail(account.email)
    if (account.accountId?.trim()) return maskAccountId(account.accountId)
    return '••••••••'
  }
  return account.email?.trim() || account.accountId?.trim() || 'Unnamed account'
}

function getQuotaStatusColor(remaining: number | null): { text: string; bg: string; fill: string } {
  if (remaining == null) {
    return { text: 'text-white/40', bg: 'bg-white/[0.06]', fill: 'bg-white/20' }
  }
  if (remaining > 30) {
    return { text: 'text-emerald-400', bg: 'bg-emerald-500/10', fill: 'bg-emerald-500' }
  }
  if (remaining > 10) {
    return { text: 'text-amber-400', bg: 'bg-amber-500/10', fill: 'bg-amber-500' }
  }
  return { text: 'text-rose-400', bg: 'bg-rose-500/10', fill: 'bg-rose-500' }
}

export function TrayFlyout() {
  const { data, loading, setData, saveAppSettings } = useAppData()
  const { privacyMode, setPrivacyMode } = usePrivacy()
  const [activeTab, setActiveTab] = useState<'all' | 'codex' | 'gemini'>('all')
  const [switchingId, setSwitchingId] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)
  const [singleRefreshingId, setSingleRefreshingId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  // Sync privacy mode with server settings
  useEffect(() => {
    const serverPrivacy = data?.appSettings?.privacyMode
    if (serverPrivacy !== undefined && serverPrivacy !== privacyMode) {
      setPrivacyMode(serverPrivacy)
      try {
        localStorage.setItem('switchai:privacy-mode', String(serverPrivacy))
      } catch {
        // ignore
      }
    }
  }, [data?.appSettings?.privacyMode, privacyMode, setPrivacyMode])

  // Shared ticker for countdowns
  useSharedTicker(true)

  // Auto-hide on Escape or blur
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        api.hideTrayFlyout().catch(() => {})
      }
    }
    const handleBlur = () => {
      api.hideTrayFlyout().catch(() => {})
    }
    window.addEventListener('keydown', handleKeyDown)
    window.addEventListener('blur', handleBlur)
    return () => {
      window.removeEventListener('keydown', handleKeyDown)
      window.removeEventListener('blur', handleBlur)
    }
  }, [])

  const rawEnabledProviders = data?.appSettings?.enabledProviders
  const enabledProviders = useMemo<AccountProvider[]>(() => {
    if (Array.isArray(rawEnabledProviders) && rawEnabledProviders.length > 0) {
      const filtered = rawEnabledProviders.filter((p): p is AccountProvider => p === 'codex' || p === 'gemini')
      if (filtered.length > 0) return filtered
    }
    return ['codex', 'gemini']
  }, [rawEnabledProviders])

  const accounts = data?.accounts
  const hiddenAccountIds = data?.appSettings?.hiddenAccountIds
  const visibleAccounts = useMemo(() => {
    if (!accounts) return []
    const hiddenSet = new Set(hiddenAccountIds ?? [])
    return accounts
      .filter((a) => !hiddenSet.has(a.id))
      .filter((a) => {
        const prov = a.provider ?? 'codex'
        return enabledProviders.includes(prov)
      })
  }, [accounts, hiddenAccountIds, enabledProviders])

  const filteredAccounts = useMemo(() => {
    if (activeTab === 'all') return visibleAccounts
    return visibleAccounts.filter((a) => (a.provider ?? 'codex') === activeTab)
  }, [visibleAccounts, activeTab])

  // Best recommended accounts per provider
  const bestCodex = useMemo(() => {
    const codexList = visibleAccounts.filter((a) => (a.provider ?? 'codex') === 'codex')
    return recommendedAccount(codexList)
  }, [visibleAccounts])

  const bestGemini = useMemo(() => {
    const geminiList = visibleAccounts.filter((a) => a.provider === 'gemini')
    return recommendedAccount(geminiList)
  }, [visibleAccounts])

  const isCodexOptimal = Boolean(
    bestCodex && data?.activeAccountId && bestCodex.id === data.activeAccountId
  )

  const isGeminiOptimal = Boolean(
    bestGemini && data?.activeGeminiAccountId && bestGemini.id === data.activeGeminiAccountId
  )

  const handleSwitchAccount = useCallback(
    async (account: Account) => {
      const isGemini = account.provider === 'gemini'
      const activeId = isGemini ? data?.activeGeminiAccountId : data?.activeAccountId
      if (activeId === account.id || account.tokenHealth?.status === 'needs_relogin') {
        return
      }

      setSwitchingId(account.id)
      setError(null)
      try {
        const res = isGemini
          ? await api.switchActiveAccountAndRestartAntigravity(account.id)
          : await api.switchActiveAccountAndRestartCodex(account.id)
        setData(res.state)
        if (res.restartWarning) {
          setError(res.restartWarning)
        }
      } catch (err) {
        setError(describeIpcError(err))
      } finally {
        setSwitchingId(null)
      }
    },
    [data?.activeAccountId, data?.activeGeminiAccountId, setData]
  )

  const handleRefreshAll = useCallback(async () => {
    if (refreshing) return
    setRefreshing(true)
    setError(null)
    try {
      const targetProvider = activeTab === 'all' ? undefined : activeTab
      const res = await api.refreshAllQuotas(targetProvider)
      if (res?.state) {
        setData(res.state)
      }
    } catch (err) {
      setError(describeIpcError(err))
    } finally {
      setRefreshing(false)
    }
  }, [refreshing, activeTab, setData])

  const handleRefreshSingle = useCallback(
    async (e: React.MouseEvent, accountId: string) => {
      e.stopPropagation()
      if (singleRefreshingId) return
      setSingleRefreshingId(accountId)
      try {
        const res = await api.refreshAccountQuota(accountId)
        if (res?.account) {
          setData((prev) => {
            if (!prev) return prev
            const idx = prev.accounts.findIndex((a) => a.id === accountId)
            if (idx === -1) return prev
            const accounts = [...prev.accounts]
            accounts[idx] = res.account
            return { ...prev, accounts }
          })
        }
      } catch (err) {
        setError(describeIpcError(err))
      } finally {
        setSingleRefreshingId(null)
      }
    },
    [singleRefreshingId, setData]
  )

  const handleOpenMain = useCallback(() => {
    api.showMainWindow().catch(() => {})
  }, [])

  const handleClose = useCallback(() => {
    api.hideTrayFlyout().catch(() => {})
  }, [])

  const togglePrivacy = useCallback(() => {
    const next = !privacyMode
    setPrivacyMode(next)
    try {
      localStorage.setItem('switchai:privacy-mode', String(next))
    } catch {
      // ignore
    }
    if (data && data.appSettings.privacyMode !== next) {
      void saveAppSettings({
        ...data.appSettings,
        privacyMode: next
      })
    }
  }, [privacyMode, setPrivacyMode, data, saveAppSettings])

  // Determine which recommendation to feature
  const heroRecommendation = useMemo(() => {
    if (activeTab === 'codex') {
      if (!bestCodex) return null
      return {
        account: bestCodex,
        isOptimal: isCodexOptimal,
        providerLabel: 'ChatGPT'
      }
    }
    if (activeTab === 'gemini') {
      if (!bestGemini) return null
      return {
        account: bestGemini,
        isOptimal: isGeminiOptimal,
        providerLabel: 'Antigravity'
      }
    }
    // 'all' tab: prioritize a provider that is NOT yet optimal
    if (bestCodex && !isCodexOptimal) {
      return {
        account: bestCodex,
        isOptimal: false,
        providerLabel: 'ChatGPT'
      }
    }
    if (bestGemini && !isGeminiOptimal) {
      return {
        account: bestGemini,
        isOptimal: false,
        providerLabel: 'Antigravity'
      }
    }
    // If both are optimal, show one with optimal badge
    if (bestCodex && isCodexOptimal) {
      return {
        account: bestCodex,
        isOptimal: true,
        providerLabel: 'ChatGPT'
      }
    }
    if (bestGemini && isGeminiOptimal) {
      return {
        account: bestGemini,
        isOptimal: true,
        providerLabel: 'Antigravity'
      }
    }
    return null
  }, [activeTab, bestCodex, bestGemini, isCodexOptimal, isGeminiOptimal])

  return (
    <div className="w-full h-full p-2 select-none overflow-hidden flex flex-col font-sans antialiased text-[#f3f4f6]">
      <div className="w-full h-full flex flex-col rounded-2xl bg-[#0c1017]/95 border border-white/[0.08] shadow-[0_20px_50px_rgba(0,0,0,0.85)] backdrop-blur-2xl overflow-hidden ring-1 ring-white/[0.05]">
        {/* Header */}
        <div className="flex items-center justify-between px-3.5 pt-3 pb-2.5 border-b border-white/[0.06] bg-white/[0.02]">
          <div className="flex items-center gap-2">
            <img src={appLogo} alt="SwitchAI" className="w-5 h-5 rounded-md shadow-sm" />
            <div className="flex items-center gap-1.5">
              <span className="font-semibold text-xs text-white tracking-tight">SwitchAI</span>
              <span className="text-[10px] font-medium px-1.5 py-0.2 rounded-full bg-blue-500/10 text-blue-400 border border-blue-500/20">
                Flyout
              </span>
            </div>
          </div>

          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={togglePrivacy}
              title={privacyMode ? 'Privacy Mode On (emails masked)' : 'Privacy Mode Off'}
              className="w-7 h-7 rounded-lg flex items-center justify-center text-white/60 hover:text-white hover:bg-white/[0.08] active:scale-95 transition-all cursor-pointer"
            >
              {privacyMode ? <EyeOff size={13} className="text-blue-400" /> : <Eye size={13} />}
            </button>

            <button
              type="button"
              onClick={handleRefreshAll}
              disabled={refreshing}
              title="Refresh Quotas"
              className="w-7 h-7 rounded-lg flex items-center justify-center text-white/60 hover:text-white hover:bg-white/[0.08] active:scale-95 transition-all cursor-pointer disabled:opacity-40"
            >
              <RefreshCw size={13} className={refreshing ? 'animate-spin text-blue-400' : ''} />
            </button>

            <button
              type="button"
              onClick={handleOpenMain}
              title="Open full SwitchAI manager"
              className="w-7 h-7 rounded-lg flex items-center justify-center text-white/60 hover:text-white hover:bg-white/[0.08] active:scale-95 transition-all cursor-pointer"
            >
              <ExternalLink size={13} />
            </button>

            <button
              type="button"
              onClick={handleClose}
              title="Close (Esc)"
              className="w-7 h-7 rounded-lg flex items-center justify-center text-white/40 hover:text-white hover:bg-white/[0.08] active:scale-95 transition-all cursor-pointer"
            >
              <X size={14} />
            </button>
          </div>
        </div>

        {/* Provider Switcher Tabs (if multiple enabled) */}
        {enabledProviders.length > 1 && (
          <div className="px-3 pt-2 pb-1 flex items-center gap-1 border-b border-white/[0.04] bg-white/[0.01]">
            <button
              type="button"
              onClick={() => setActiveTab('all')}
              className={`flex-1 py-1 px-2 rounded-lg text-[11px] font-medium transition-all cursor-pointer text-center ${
                activeTab === 'all'
                  ? 'bg-white/[0.1] text-white shadow-sm'
                  : 'text-white/50 hover:text-white/80 hover:bg-white/[0.04]'
              }`}
            >
              All ({visibleAccounts.length})
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('codex')}
              className={`flex-1 py-1 px-2 rounded-lg text-[11px] font-medium transition-all cursor-pointer text-center ${
                activeTab === 'codex'
                  ? 'bg-emerald-500/15 text-emerald-300 border border-emerald-500/30 shadow-sm'
                  : 'text-white/50 hover:text-white/80 hover:bg-white/[0.04]'
              }`}
            >
              ChatGPT
            </button>
            <button
              type="button"
              onClick={() => setActiveTab('gemini')}
              className={`flex-1 py-1 px-2 rounded-lg text-[11px] font-medium transition-all cursor-pointer text-center ${
                activeTab === 'gemini'
                  ? 'bg-purple-500/15 text-purple-300 border border-purple-500/30 shadow-sm'
                  : 'text-white/50 hover:text-white/80 hover:bg-white/[0.04]'
              }`}
            >
              Antigravity
            </button>
          </div>
        )}

        {/* Quick Error Banner if any */}
        {error && (
          <div className="mx-3 mt-2 px-2.5 py-1.5 rounded-lg bg-rose-500/10 border border-rose-500/20 text-rose-300 text-[11px] flex items-center justify-between">
            <span className="truncate">{error}</span>
            <button
              type="button"
              onClick={() => setError(null)}
              className="text-rose-400 hover:text-rose-200 ml-2"
            >
              ✕
            </button>
          </div>
        )}

        {/* Hero: 1-Click Switch to Best / Recommended Action */}
        {heroRecommendation && (
          <div className="px-3 pt-2.5 pb-1">
            <div
              className={`p-2.5 rounded-xl border transition-all ${
                heroRecommendation.isOptimal
                  ? 'bg-emerald-950/20 border-emerald-500/20'
                  : 'bg-gradient-to-r from-blue-950/40 via-indigo-950/30 to-blue-900/20 border-blue-500/30 shadow-sm shadow-blue-500/10 ring-1 ring-blue-400/20'
              }`}
            >
              <div className="flex items-center justify-between mb-1.5">
                <div className="flex items-center gap-1.5">
                  {heroRecommendation.isOptimal ? (
                    <ShieldCheck size={13} className="text-emerald-400" />
                  ) : (
                    <Sparkles size={13} className="text-amber-400 animate-pulse" />
                  )}
                  <span className="text-[11px] font-semibold tracking-tight text-white">
                    {heroRecommendation.isOptimal ? 'Optimal Account Active' : 'Switch to Best'}
                  </span>
                  <span className="text-[10px] px-1 py-0.2 rounded bg-white/[0.08] text-white/60">
                    {heroRecommendation.providerLabel}
                  </span>
                </div>

                {!heroRecommendation.isOptimal && (
                  <button
                    type="button"
                    onClick={() => handleSwitchAccount(heroRecommendation.account)}
                    disabled={switchingId === heroRecommendation.account.id}
                    className="px-2.5 py-1 rounded-lg bg-blue-500 hover:bg-blue-400 active:scale-95 text-white text-[11px] font-semibold flex items-center gap-1 shadow-sm transition-all cursor-pointer disabled:opacity-50"
                  >
                    {switchingId === heroRecommendation.account.id ? (
                      <Loader2 size={11} className="animate-spin" />
                    ) : (
                      <Zap size={11} />
                    )}
                    Switch Now
                  </button>
                )}
              </div>

              <div className="flex items-center justify-between text-[11px]">
                <span className="text-white/80 font-medium truncate max-w-[210px]">
                  {getAccountDisplayName(heroRecommendation.account, privacyMode)}
                </span>
                <span className="text-emerald-400 font-semibold tabular-nums">
                  {(() => {
                    const rem = accountRemainingPercent(heroRecommendation.account)
                    return rem != null ? `${Math.round(rem)}% left` : 'Ready'
                  })()}
                </span>
              </div>
            </div>
          </div>
        )}

        {/* Account List */}
        <div className="flex-1 overflow-y-auto px-3 py-2 space-y-1.5 min-h-0">
          {loading && visibleAccounts.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 gap-2 text-white/40">
              <Loader2 size={20} className="animate-spin text-blue-400" />
              <span className="text-xs">Loading accounts...</span>
            </div>
          ) : filteredAccounts.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 gap-2 text-center text-white/40 px-4">
              <p className="text-xs">No accounts available in this view.</p>
              <button
                type="button"
                onClick={handleOpenMain}
                className="mt-1 text-[11px] text-blue-400 hover:underline cursor-pointer flex items-center gap-1"
              >
                Open SwitchAI to add an account <ExternalLink size={11} />
              </button>
            </div>
          ) : (
            filteredAccounts.map((account) => {
              const isGemini = account.provider === 'gemini'
              const activeId = isGemini ? data?.activeGeminiAccountId : data?.activeAccountId
              const isActive = activeId === account.id
              const needsRelogin = account.tokenHealth?.status === 'needs_relogin'
              const remaining = accountRemainingPercent(account)
              const colors = getQuotaStatusColor(remaining)
              const resets = [account.quota?.primary?.resetAt, account.quota?.secondary?.resetAt]
                .filter((ts): ts is number => ts != null && ts * 1000 > Date.now())
              resets.sort((a, b) => a - b)
              const nextReset = resets[0] ?? account.quota?.primary?.resetAt ?? account.quota?.secondary?.resetAt
              const plan = formatSubscriptionPlan(account.subscriptionPlan, account.provider)
              const isSwitching = switchingId === account.id
              const isSingleRefreshing = singleRefreshingId === account.id

              return (
                <div
                  key={account.id}
                  onClick={() => handleSwitchAccount(account)}
                  className={`group relative p-2.5 rounded-xl border transition-all cursor-pointer select-none ${
                    isActive
                      ? 'bg-blue-950/25 border-blue-500/40 ring-1 ring-blue-500/20 shadow-sm'
                      : needsRelogin
                      ? 'bg-rose-950/10 border-rose-500/20 opacity-80 hover:opacity-100 hover:bg-rose-950/20'
                      : 'bg-white/[0.025] border-white/[0.06] hover:bg-white/[0.055] hover:border-white/[0.12]'
                  }`}
                >
                  {/* Row 1: Active dot, Name, Badges, Switch status */}
                  <div className="flex items-center justify-between gap-2 mb-1.5">
                    <div className="flex items-center gap-2 min-w-0">
                      {isActive ? (
                        <div
                          className="w-2 h-2 rounded-full bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.8)] shrink-0"
                          title="Active account"
                        />
                      ) : (
                        <div className="w-2 h-2 rounded-full bg-white/20 shrink-0 group-hover:bg-white/40 transition-colors" />
                      )}

                      <span
                        className={`text-xs font-semibold truncate ${
                          isActive ? 'text-white' : 'text-white/80 group-hover:text-white'
                        }`}
                        title={account.email || account.accountId || ''}
                      >
                        {getAccountDisplayName(account, privacyMode)}
                      </span>
                    </div>

                    <div className="flex items-center gap-1.5 shrink-0">
                      {/* Provider badge */}
                      <span
                        className={`text-[9px] font-semibold px-1.5 py-0.2 rounded tracking-wide uppercase ${
                          isGemini
                            ? 'bg-purple-500/15 text-purple-300 border border-purple-500/20'
                            : 'bg-emerald-500/15 text-emerald-300 border border-emerald-500/20'
                        }`}
                      >
                        {isGemini ? 'Antigravity' : 'ChatGPT'}
                      </span>

                      {/* Plan pill */}
                      {plan && (
                        <span className="text-[9px] font-medium px-1.5 py-0.2 rounded bg-white/[0.06] text-white/60 border border-white/[0.06]">
                          {plan}
                        </span>
                      )}

                      {/* Active check or Switch status */}
                      {isActive ? (
                        <span className="flex items-center gap-0.5 text-[10px] font-semibold text-emerald-400 ml-0.5">
                          <Check size={11} strokeWidth={3} />
                        </span>
                      ) : isSwitching ? (
                        <Loader2 size={12} className="animate-spin text-blue-400 ml-0.5" />
                      ) : null}
                    </div>
                  </div>

                  {/* Warning if needs relogin */}
                  {needsRelogin && (
                    <div
                      onClick={(e) => {
                        e.stopPropagation()
                        handleOpenMain()
                      }}
                      className="flex items-center justify-between gap-1.5 mb-1.5 text-[10px] text-rose-300 bg-rose-500/10 px-2 py-1 rounded-md border border-rose-500/20 hover:bg-rose-500/20 transition-colors cursor-pointer"
                      title="Open SwitchAI to re-authenticate"
                    >
                      <div className="flex items-center gap-1.5 min-w-0">
                        <AlertTriangle size={11} className="text-rose-400 shrink-0" />
                        <span className="truncate">Session expired · Re-login required</span>
                      </div>
                      <ExternalLink size={10} className="shrink-0 text-rose-400" />
                    </div>
                  )}

                  {/* Quota Progress Bar */}
                  <div className="space-y-1">
                    <div className="h-1.5 w-full bg-white/[0.06] rounded-full overflow-hidden">
                      <div
                        className={`h-full rounded-full transition-all duration-300 ${colors.fill}`}
                        style={{ width: `${Math.max(0, Math.min(100, remaining ?? 0))}%` }}
                      />
                    </div>

                    <div className="flex items-center justify-between text-[10px] text-white/50 pt-0.5">
                      <span className={`font-semibold tabular-nums ${colors.text}`}>
                        {remaining != null ? `${Math.round(remaining)}% remaining` : 'Quota unavailable'}
                      </span>

                      <div className="flex items-center gap-2">
                        {nextReset && (
                          <span className="flex items-center gap-1 text-white/40">
                            <Clock size={9} />
                            reset {formatTimeUntil(nextReset)}
                          </span>
                        )}

                        <button
                          type="button"
                          onClick={(e) => handleRefreshSingle(e, account.id)}
                          title="Refresh quota for this account"
                          className="p-1 rounded text-white/30 group-hover:text-white/60 hover:!text-white hover:bg-white/[0.08] transition-all cursor-pointer"
                        >
                          <RefreshCw
                            size={10}
                            className={isSingleRefreshing ? 'animate-spin text-blue-400' : ''}
                          />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              )
            })
          )}
        </div>

        {/* Footer */}
        <div className="px-3.5 py-2.5 border-t border-white/[0.06] bg-white/[0.02] flex items-center justify-between text-[11px] text-white/50">
          <div className="flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.8)]" />
            <span>
              {visibleAccounts.length} {visibleAccounts.length === 1 ? 'account' : 'accounts'}
            </span>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleOpenMain}
              className="text-white/70 hover:text-white hover:underline transition-colors flex items-center gap-1 font-medium cursor-pointer"
            >
              Open Manager <ExternalLink size={11} />
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
