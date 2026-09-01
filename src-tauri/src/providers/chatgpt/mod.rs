pub mod oauth;
pub mod quota;
pub mod sync;

use crate::dto::AntigravitySurfaceDto;
use crate::errors::AppResult;
use crate::models::{Account, AccountProvider};
use crate::providers::Provider;

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

    fn sync_active_account(&self, account: Option<&Account>) -> AppResult<()> {
        crate::refresh_service::write_account_auth(account)
    }

    fn restart_process(&self) -> AppResult<()> {
        sync::restart_codex_process()
    }

    fn detect_surfaces(&self) -> Vec<AntigravitySurfaceDto> {
        Vec::new()
    }
}
