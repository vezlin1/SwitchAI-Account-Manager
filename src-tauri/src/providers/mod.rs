pub mod chatgpt;
pub mod gemini;

use crate::dto::AntigravitySurfaceDto;
use crate::errors::AppResult;
use crate::models::{Account, AccountProvider};

pub use chatgpt::ChatGptProvider;
pub use gemini::GeminiProvider;

/// Common interface for AI account providers.
pub trait Provider: Send + Sync {
    /// Provider enum discriminant
    fn provider(&self) -> AccountProvider;

    /// Machine-readable provider key
    fn id_str(&self) -> &'static str;

    /// Human-readable display title
    fn display_name(&self) -> &'static str;

    /// Write credentials of the active account to the provider's local auth store
    fn sync_active_account(&self, account: Option<&Account>) -> AppResult<()>;

    /// Restart the primary target application for this provider
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
