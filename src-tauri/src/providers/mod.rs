pub mod chatgpt;
pub mod gemini;

use std::future::Future;
use std::pin::Pin;

use crate::dto::AntigravitySurfaceDto;
use crate::errors::AppResult;
use crate::models::{Account, AccountProvider};

pub use chatgpt::ChatGptProvider;
pub use gemini::GeminiProvider;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Common interface for AI account providers.
pub trait Provider: Send + Sync {
    /// Provider enum discriminant
    fn provider(&self) -> AccountProvider;

    /// Machine-readable provider key
    fn id_str(&self) -> &'static str;

    /// Human-readable display title
    fn display_name(&self) -> &'static str;

    /// Write credentials of the active account to the provider's local auth store
    fn sync_active_account<'a>(
        &'a self,
        account: Option<&'a Account>,
    ) -> BoxFuture<'a, AppResult<()>>;

    /// Refresh quota and status for an account belonging to this provider
    fn refresh_account<'a>(
        &'a self,
        state: &'a std::sync::Arc<crate::app_state::SharedState>,
        account_id: &'a str,
    ) -> BoxFuture<'a, AppResult<crate::refresh_service::AccountRefreshOutcome>>;

    /// Restart the primary target application for this provider
    fn restart_target_process<'a>(&'a self) -> BoxFuture<'a, AppResult<()>>;

    /// Synchronous restart helper
    fn restart_process(&self) -> AppResult<()>;

    /// Detect installed surfaces / tools for this provider
    fn detect_surfaces(&self) -> Vec<AntigravitySurfaceDto>;
}

static CHATGPT_PROVIDER: ChatGptProvider = ChatGptProvider;
static GEMINI_PROVIDER: GeminiProvider = GeminiProvider;

/// Resolve provider adapter instance for the given provider enum
pub fn get_provider(provider: AccountProvider) -> &'static dyn Provider {
    match provider {
        AccountProvider::Codex => &CHATGPT_PROVIDER,
        AccountProvider::Gemini => &GEMINI_PROVIDER,
    }
}
