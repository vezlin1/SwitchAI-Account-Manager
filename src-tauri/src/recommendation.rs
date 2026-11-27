//! The single place that decides which account is the best one to switch to.
//!
//! The tray, the tooltip and the window all ask this module, so they can never
//! disagree. The score combines three things:
//! - the tightest remaining quota window that matters for the user,
//! - the approximate size of the plan (30% of a Pro plan is more work than 90%
//!   of a Free plan),
//! - "use it or lose it": a long window that resets within a day is worth
//!   spending before quota on an account whose window resets next week.
//!
//! A window whose reset time has passed since the last check is projected as
//! fully available instead of being treated as stale.

use serde::Serialize;

use crate::models::{
    Account, AccountProvider, AppData, AppSettings, QuotaWindow, TokenHealthStatus,
};

const FRESH_SECONDS: i64 = 60 * 60;
const CLOCK_SKEW_SECONDS: i64 = 60;
const DAY_SECONDS: i64 = 24 * 60 * 60;
/// Below this the account cannot do meaningful work and is never recommended.
const EXHAUSTED_REMAINING_PERCENT: f64 = 1.0;
/// Urgency only applies when the expiring window still holds this much quota.
const URGENCY_MIN_REMAINING_PERCENT: f64 = 10.0;
/// Above this urgency factor the expiring reset is what decided the pick.
const URGENCY_REASON_FACTOR: f64 = 1.25;

pub const GEMINI_QUOTA_FOCUS_GEMINI: &str = "gemini";
pub const GEMINI_QUOTA_FOCUS_CLAUDE_AND_GPT: &str = "claude_and_gpt";
pub const GEMINI_QUOTA_FOCUS_BOTH: &str = "both";
pub const GEMINI_QUOTA_FOCUS_VALUES: [&str; 3] = [
    GEMINI_QUOTA_FOCUS_GEMINI,
    GEMINI_QUOTA_FOCUS_CLAUDE_AND_GPT,
    GEMINI_QUOTA_FOCUS_BOTH,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaFocus {
    /// Codex accounts, or Antigravity accounts judged by Gemini models.
    Primary,
    /// Antigravity accounts judged by the Claude and GPT model group.
    Partner,
    /// Antigravity accounts judged by whichever group is tighter.
    Both,
}

pub fn quota_focus(settings: &AppSettings, provider: AccountProvider) -> QuotaFocus {
    match provider {
        AccountProvider::Codex => QuotaFocus::Primary,
        AccountProvider::Gemini => match settings.gemini_quota_focus.as_str() {
            GEMINI_QUOTA_FOCUS_CLAUDE_AND_GPT => QuotaFocus::Partner,
            GEMINI_QUOTA_FOCUS_BOTH => QuotaFocus::Both,
            _ => QuotaFocus::Primary,
        },
    }
}

/// The quota windows that matter for this account under the given focus.
pub fn relevant_windows(account: &Account, focus: QuotaFocus) -> Vec<&QuotaWindow> {
    let Some(quota) = account.quota.as_ref() else {
        return Vec::new();
    };
    let primary = [&quota.primary, &quota.secondary];
    let partner = quota
        .claude_and_gpt
        .as_ref()
        .map(|windows| [&windows.primary, &windows.secondary]);
    let windows: Vec<&QuotaWindow> = match (account.provider, focus) {
        (AccountProvider::Codex, _) | (_, QuotaFocus::Primary) => primary.to_vec(),
        (_, QuotaFocus::Partner) => partner.map(|w| w.to_vec()).unwrap_or_default(),
        (_, QuotaFocus::Both) => primary
            .into_iter()
            .chain(partner.into_iter().flatten())
            .collect(),
    };
    windows
        .into_iter()
        .filter(|window| window.used_percent.is_some())
        .collect()
}

fn valid_used(window: &QuotaWindow) -> Option<f64> {
    window
        .used_percent
        .filter(|used| used.is_finite() && (0.0..=100.0).contains(used))
}

/// Remaining percent of the tightest relevant window as last reported by the
/// service, without projection. Used for labels and alerts.
pub fn remaining_percent(account: &Account, focus: QuotaFocus) -> Option<f64> {
    relevant_windows(account, focus)
        .into_iter()
        .filter_map(valid_used)
        .map(|used| (100.0 - used).clamp(0.0, 100.0))
        .min_by(f64::total_cmp)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectiveQuota {
    /// Remaining percent of the tightest relevant window.
    pub remaining: f64,
    /// At least one window has reset since it was last checked and is assumed
    /// to be full again.
    pub projected: bool,
    /// Multiplier >= 1 rewarding a long window that resets within a day.
    pub urgency: f64,
}

/// Quota usable for a decision right now, or None when it is missing, invalid
/// or too old to trust.
pub fn effective_quota(account: &Account, focus: QuotaFocus, now: i64) -> Option<EffectiveQuota> {
    let quota = account.quota.as_ref()?;
    let windows = relevant_windows(account, focus);
    if windows.is_empty() {
        return None;
    }

    let mut remaining = f64::INFINITY;
    let mut projected = false;
    let mut urgency: f64 = 1.0;
    for window in windows {
        let used = valid_used(window)?;
        let checked_at = window.fetched_at.unwrap_or(quota.fetched_at);
        let reset_passed = window
            .reset_at
            .is_some_and(|reset| reset <= now && reset > checked_at);
        let window_remaining = if reset_passed {
            projected = true;
            100.0
        } else {
            let age = now.saturating_sub(checked_at);
            if !(-CLOCK_SKEW_SECONDS..FRESH_SECONDS).contains(&age) {
                return None;
            }
            (100.0 - used).clamp(0.0, 100.0)
        };
        remaining = remaining.min(window_remaining);

        let is_long_window = window
            .limit_window_seconds
            .is_some_and(|length| length >= DAY_SECONDS);
        if !reset_passed
            && is_long_window
            && window_remaining >= URGENCY_MIN_REMAINING_PERCENT
            && let Some(reset_at) = window.reset_at
        {
            let until_reset = reset_at.saturating_sub(now);
            if (0..DAY_SECONDS).contains(&until_reset) {
                urgency = urgency.max(1.0 + (1.0 - until_reset as f64 / DAY_SECONDS as f64));
            }
        }
    }

    Some(EffectiveQuota {
        remaining,
        projected,
        urgency,
    })
}

/// Approximate quota size relative to a standard paid plan (Plus / Google AI
/// Pro). Only the ratio between accounts matters. Unknown plans count as 1.
pub fn plan_weight(account: &Account) -> f64 {
    let label = match account.provider {
        AccountProvider::Codex => crate::subscription::saved_plan_label(
            account.subscription_plan.as_deref(),
            account
                .quota
                .as_ref()
                .and_then(|quota| quota.plan_type.as_deref()),
        ),
        AccountProvider::Gemini => account.subscription_plan.clone(),
    };
    let Some(label) = label else {
        return 1.0;
    };
    let label = label.trim().to_ascii_lowercase();
    match account.provider {
        AccountProvider::Codex => {
            if label.starts_with("free") {
                0.25
            } else if label == "pro 100" {
                5.0
            } else if label.starts_with("pro") {
                20.0
            } else {
                1.0
            }
        }
        AccountProvider::Gemini => {
            if label.contains("ultra") {
                4.0
            } else if label.contains("pro") {
                1.0
            } else if label.contains("plus") {
                0.5
            } else if label.contains("free") {
                0.25
            } else {
                1.0
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationReason {
    /// It simply has the most quota left.
    MostRemaining,
    /// A long window resets soon; its quota would otherwise be lost.
    ResetsSoon,
    /// A lower percentage on a larger plan is more absolute quota.
    LargerPlan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    pub account_id: String,
    pub remaining: f64,
    pub projected: bool,
    pub reason: RecommendationReason,
}

fn is_candidate(account: &Account, data: &AppData, provider: AccountProvider) -> bool {
    account.provider == provider
        && !data.app_settings.hidden_account_ids.contains(&account.id)
        && matches!(
            account.token_health.status,
            TokenHealthStatus::Healthy | TokenHealthStatus::Refreshed
        )
        && account.last_error.is_none()
}

pub fn recommend(data: &AppData, provider: AccountProvider, now: i64) -> Option<Recommendation> {
    let focus = quota_focus(&data.app_settings, provider);
    let candidates: Vec<(&Account, EffectiveQuota, f64)> = data
        .accounts
        .iter()
        .filter(|account| is_candidate(account, data, provider))
        .filter_map(|account| {
            let quota = effective_quota(account, focus, now)?;
            (quota.remaining >= EXHAUSTED_REMAINING_PERCENT).then(|| {
                let score = plan_weight(account) * quota.remaining * quota.urgency;
                (account, quota, score)
            })
        })
        .collect();

    let (best, best_quota, _) = candidates
        .iter()
        .max_by(|left, right| left.2.total_cmp(&right.2))?;
    let most_remaining = candidates
        .iter()
        .map(|(_, quota, _)| quota.remaining)
        .fold(f64::NEG_INFINITY, f64::max);

    let reason = if best_quota.remaining >= most_remaining {
        RecommendationReason::MostRemaining
    } else if best_quota.urgency >= URGENCY_REASON_FACTOR {
        RecommendationReason::ResetsSoon
    } else {
        RecommendationReason::LargerPlan
    };

    Some(Recommendation {
        account_id: best.id.clone(),
        remaining: best_quota.remaining,
        projected: best_quota.projected,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{QuotaInfo, QuotaWindows, TokenHealth};

    const NOW: i64 = 1_000_000;
    const HOUR: i64 = 60 * 60;
    const WEEK: i64 = 7 * DAY_SECONDS;

    fn window(used: f64, length: i64, reset_at: i64) -> QuotaWindow {
        QuotaWindow {
            used_percent: Some(used),
            limit_window_seconds: Some(length),
            reset_at: Some(reset_at),
            fetched_at: Some(NOW - 60),
        }
    }

    fn account(
        id: &str,
        provider: AccountProvider,
        plan: &str,
        weekly_used: f64,
        weekly_reset_in: i64,
    ) -> Account {
        Account {
            id: id.to_string(),
            provider,
            email: Some(format!("{id}@example.com")),
            account_id: None,
            provider_project_id: None,
            subscription_expires_at: None,
            subscription_plan: Some(plan.to_string()),
            subscription_detected_at: None,
            subscription_checked_at: None,
            subscription_next_check_at: None,
            subscription_endpoint_hint: None,
            tokens: crate::models::Tokens::default(),
            token_expires_at: None,
            tokens_updated_at: None,
            token_health: TokenHealth {
                status: TokenHealthStatus::Healthy,
                last_checked_at: None,
                last_refreshed_at: None,
                last_error: None,
            },
            quota: Some(QuotaInfo {
                plan_type: None,
                primary: window(0.0, 5 * HOUR, NOW + 2 * HOUR),
                secondary: window(weekly_used, WEEK, NOW + weekly_reset_in),
                claude_and_gpt: None,
                fetched_at: NOW - 60,
            }),
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: 1,
            last_login_at: 1,
            last_error: None,
            subscription_error: None,
        }
    }

    fn data(accounts: Vec<Account>) -> AppData {
        AppData {
            accounts,
            ..AppData::default()
        }
    }

    #[test]
    fn same_plan_and_distant_resets_pick_the_most_remaining() {
        let data = data(vec![
            account("low", AccountProvider::Codex, "Plus", 80.0, 5 * DAY_SECONDS),
            account(
                "high",
                AccountProvider::Codex,
                "Plus",
                10.0,
                5 * DAY_SECONDS,
            ),
        ]);
        let pick = recommend(&data, AccountProvider::Codex, NOW).expect("pick");
        assert_eq!(pick.account_id, "high");
        assert_eq!(pick.reason, RecommendationReason::MostRemaining);
        assert_eq!(pick.remaining, 90.0);
    }

    #[test]
    fn quota_that_expires_soon_is_spent_first() {
        let data = data(vec![
            account(
                "later",
                AccountProvider::Codex,
                "Plus",
                20.0,
                6 * DAY_SECONDS,
            ),
            account("expiring", AccountProvider::Codex, "Plus", 30.0, HOUR),
        ]);
        let pick = recommend(&data, AccountProvider::Codex, NOW).expect("pick");
        assert_eq!(pick.account_id, "expiring");
        assert_eq!(pick.reason, RecommendationReason::ResetsSoon);
    }

    #[test]
    fn larger_plan_wins_with_a_lower_percentage() {
        let data = data(vec![
            account(
                "plus",
                AccountProvider::Codex,
                "Plus",
                10.0,
                5 * DAY_SECONDS,
            ),
            account(
                "pro",
                AccountProvider::Codex,
                "Pro 200",
                70.0,
                5 * DAY_SECONDS,
            ),
        ]);
        let pick = recommend(&data, AccountProvider::Codex, NOW).expect("pick");
        assert_eq!(pick.account_id, "pro");
        assert_eq!(pick.reason, RecommendationReason::LargerPlan);
    }

    #[test]
    fn passed_reset_is_projected_as_full_instead_of_stale() {
        let mut reset = account(
            "reset",
            AccountProvider::Codex,
            "Plus",
            100.0,
            5 * DAY_SECONDS,
        );
        let quota = reset.quota.as_mut().expect("quota");
        quota.fetched_at = NOW - 3 * HOUR;
        quota.primary.fetched_at = Some(NOW - 3 * HOUR);
        quota.secondary = QuotaWindow {
            used_percent: Some(100.0),
            limit_window_seconds: Some(WEEK),
            reset_at: Some(NOW - HOUR),
            fetched_at: Some(NOW - 3 * HOUR),
        };
        quota.primary.reset_at = Some(NOW - 2 * HOUR);
        let other = account(
            "other",
            AccountProvider::Codex,
            "Plus",
            50.0,
            5 * DAY_SECONDS,
        );

        let pick = recommend(&data(vec![reset, other]), AccountProvider::Codex, NOW).expect("pick");
        assert_eq!(pick.account_id, "reset");
        assert!(pick.projected);
        assert_eq!(pick.remaining, 100.0);
    }

    #[test]
    fn stale_exhausted_hidden_and_unhealthy_accounts_are_never_recommended() {
        let mut stale = account(
            "stale",
            AccountProvider::Codex,
            "Plus",
            0.0,
            5 * DAY_SECONDS,
        );
        stale.quota.as_mut().expect("quota").secondary.fetched_at = Some(NOW - 2 * HOUR);
        let exhausted = account(
            "exhausted",
            AccountProvider::Codex,
            "Plus",
            100.0,
            5 * DAY_SECONDS,
        );
        let hidden = account(
            "hidden",
            AccountProvider::Codex,
            "Plus",
            0.0,
            5 * DAY_SECONDS,
        );
        let mut relogin = account(
            "relogin",
            AccountProvider::Codex,
            "Plus",
            0.0,
            5 * DAY_SECONDS,
        );
        relogin.token_health.status = TokenHealthStatus::NeedsRelogin;
        let mut failed = account(
            "failed",
            AccountProvider::Codex,
            "Plus",
            0.0,
            5 * DAY_SECONDS,
        );
        failed.last_error = Some("Request failed".into());

        let mut data = data(vec![stale, exhausted, hidden, relogin, failed]);
        data.app_settings.hidden_account_ids = vec!["hidden".into()];
        assert_eq!(recommend(&data, AccountProvider::Codex, NOW), None);
    }

    #[test]
    fn antigravity_focus_selects_the_model_group_that_matters() {
        let mut gemini_rich = account(
            "gemini-rich",
            AccountProvider::Gemini,
            "Google AI Pro",
            0.0,
            5 * DAY_SECONDS,
        );
        gemini_rich.quota.as_mut().expect("quota").claude_and_gpt = Some(QuotaWindows {
            primary: window(95.0, 5 * HOUR, NOW + HOUR),
            secondary: window(95.0, WEEK, NOW + 5 * DAY_SECONDS),
        });
        let mut claude_rich = account(
            "claude-rich",
            AccountProvider::Gemini,
            "Google AI Pro",
            60.0,
            5 * DAY_SECONDS,
        );
        claude_rich.quota.as_mut().expect("quota").claude_and_gpt = Some(QuotaWindows {
            primary: window(0.0, 5 * HOUR, NOW + HOUR),
            secondary: window(10.0, WEEK, NOW + 5 * DAY_SECONDS),
        });
        let mut data = data(vec![gemini_rich, claude_rich]);

        let pick =
            |data: &AppData| recommend(data, AccountProvider::Gemini, NOW).map(|r| r.account_id);
        assert_eq!(pick(&data).as_deref(), Some("gemini-rich"));
        data.app_settings.gemini_quota_focus = GEMINI_QUOTA_FOCUS_CLAUDE_AND_GPT.into();
        assert_eq!(pick(&data).as_deref(), Some("claude-rich"));
        data.app_settings.gemini_quota_focus = GEMINI_QUOTA_FOCUS_BOTH.into();
        assert_eq!(pick(&data).as_deref(), Some("claude-rich"));
    }

    #[test]
    fn providers_never_mix() {
        let data = data(vec![
            account(
                "codex",
                AccountProvider::Codex,
                "Plus",
                50.0,
                5 * DAY_SECONDS,
            ),
            account(
                "gemini",
                AccountProvider::Gemini,
                "Google AI Pro",
                0.0,
                5 * DAY_SECONDS,
            ),
        ]);
        assert_eq!(
            recommend(&data, AccountProvider::Codex, NOW)
                .map(|r| r.account_id)
                .as_deref(),
            Some("codex")
        );
        assert_eq!(
            recommend(&data, AccountProvider::Gemini, NOW)
                .map(|r| r.account_id)
                .as_deref(),
            Some("gemini")
        );
    }
}
