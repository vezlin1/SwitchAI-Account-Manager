pub mod oauth;
pub mod quota;
pub mod sync;

use crate::dto::AntigravitySurfaceDto;
use crate::errors::AppResult;
use crate::models::{Account, AccountProvider};
use crate::providers::Provider;

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

    fn sync_active_account(&self, account: Option<&Account>) -> AppResult<()> {
        if let Some(acc) = account {
            sync::write_antigravity_account_auth(acc)
        } else {
            sync::clear_antigravity_auth()
        }
    }

    fn restart_process(&self) -> AppResult<()> {
        sync::restart_antigravity_process()
    }

    fn detect_surfaces(&self) -> Vec<AntigravitySurfaceDto> {
        sync::get_antigravity_surfaces()
    }
}
