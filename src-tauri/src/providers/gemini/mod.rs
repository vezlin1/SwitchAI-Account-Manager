pub mod oauth;
pub mod quota;
pub mod sync;

use crate::dto::AntigravitySurfaceDto;
use crate::errors::AppResult;
use crate::models::{Account, AccountProvider};
use crate::providers::{BoxFuture, Provider};

pub struct GeminiProvider;

impl Provider for GeminiProvider {
    fn provider(&self) -> AccountProvider {
        AccountProvider::Gemini
    }

    fn id_str(&self) -> &'static str {
        "gemini"
    }

    fn display_name(&self) -> &'static str {
        "Google / Antigravity"
    }

    fn sync_active_account<'a>(
        &'a self,
        account: Option<&'a Account>,
    ) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            if let Some(acc) = account {
                sync::write_antigravity_account_auth(acc)
            } else {
                sync::clear_antigravity_auth()
            }
        })
    }

    fn refresh_account<'a>(
        &'a self,
        state: &'a std::sync::Arc<crate::app_state::SharedState>,
        account_id: &'a str,
    ) -> BoxFuture<'a, AppResult<crate::refresh_service::AccountRefreshOutcome>> {
        Box::pin(
            async move { crate::gemini_quota::refresh_gemini_account(state, account_id).await },
        )
    }

    fn restart_target_process<'a>(&'a self) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move { sync::restart_antigravity_process() })
    }

    fn restart_process(&self) -> AppResult<()> {
        sync::restart_antigravity_process()
    }

    fn detect_surfaces(&self) -> Vec<AntigravitySurfaceDto> {
        sync::get_antigravity_surfaces()
    }
}
