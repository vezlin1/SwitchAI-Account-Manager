pub mod oauth;
pub mod quota;
pub mod sync;

use crate::dto::AntigravitySurfaceDto;
use crate::errors::AppResult;
use crate::models::{Account, AccountProvider};
use crate::providers::{BoxFuture, Provider};

pub struct ChatGptProvider;

impl Provider for ChatGptProvider {
    fn provider(&self) -> AccountProvider {
        AccountProvider::Codex
    }

    fn id_str(&self) -> &'static str {
        "chatgpt"
    }

    fn display_name(&self) -> &'static str {
        "ChatGPT"
    }

    fn sync_active_account<'a>(
        &'a self,
        account: Option<&'a Account>,
    ) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move { crate::refresh_service::write_account_auth(account) })
    }

    fn refresh_account<'a>(
        &'a self,
        state: &'a std::sync::Arc<crate::app_state::SharedState>,
        account_id: &'a str,
    ) -> BoxFuture<'a, AppResult<crate::refresh_service::AccountRefreshOutcome>> {
        Box::pin(async move {
            crate::refresh_service::RefreshService::new(std::sync::Arc::clone(state))
                .refresh_single_account(account_id)
                .await
        })
    }

    fn restart_target_process<'a>(&'a self) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move { sync::restart_codex_process() })
    }

    fn restart_process(&self) -> AppResult<()> {
        sync::restart_codex_process()
    }

    fn detect_surfaces(&self) -> Vec<AntigravitySurfaceDto> {
        Vec::new()
    }
}
