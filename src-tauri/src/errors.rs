use serde::Serialize;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    Json {
        context: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("{context}: {source}")]
    Http {
        context: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("{context} ({status}): {details}")]
    RemoteHttp {
        context: &'static str,
        status: u16,
        retry_after_seconds: Option<i64>,
        details: String,
    },
    #[error("{context}: {source}")]
    Url {
        context: &'static str,
        #[source]
        source: url::ParseError,
    },
}

impl AppError {
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn user_message(&self) -> String {
        self.to_string()
    }

    pub fn retry_after_seconds(&self) -> Option<i64> {
        match self {
            Self::RemoteHttp {
                retry_after_seconds,
                ..
            } => *retry_after_seconds,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcErrorDto {
    pub code: String,
    pub domain: String,
    pub message: String,
    pub account_id: Option<String>,
    pub retryable: bool,
}

impl From<AppError> for IpcErrorDto {
    fn from(error: AppError) -> Self {
        let retryable = matches!(
            error,
            AppError::Http { .. }
                | AppError::RemoteHttp {
                    status: 408 | 425 | 429 | 500..=599,
                    ..
                }
        );
        let (code, domain) = match &error {
            AppError::RemoteHttp { status, .. } => (format!("remote_http_{status}"), "account"),
            AppError::Http { .. } => ("network_error".to_string(), "system"),
            AppError::Io { .. } => ("io_error".to_string(), "system"),
            AppError::Json { .. } => ("invalid_data".to_string(), "system"),
            AppError::Url { .. } => ("invalid_url".to_string(), "auth"),
            AppError::Message(_) => ("operation_failed".to_string(), "system"),
        };
        Self {
            code,
            domain: domain.to_string(),
            message: error.user_message(),
            account_id: None,
            retryable,
        }
    }
}

pub fn to_command_error(error: AppError) -> IpcErrorDto {
    error.into()
}
