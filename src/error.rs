use serde::Serialize;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    EUsage,
    EAuth,
    EConfig,
    ENotFound,
    ERateLimited,
    EUnsupported,
    ENetwork,
    EUpstream,
    EOfflineMiss,
    EShareLink,
    EStoreSchema,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EUsage => "E_USAGE",
            Self::EAuth => "E_AUTH",
            Self::EConfig => "E_CONFIG",
            Self::ENotFound => "E_NOT_FOUND",
            Self::ERateLimited => "E_RATE_LIMITED",
            Self::EUnsupported => "E_UNSUPPORTED",
            Self::ENetwork => "E_NETWORK",
            Self::EUpstream => "E_UPSTREAM",
            Self::EOfflineMiss => "E_OFFLINE_MISS",
            Self::EShareLink => "E_SHARE_LINK",
            Self::EStoreSchema => "E_STORE_SCHEMA",
        }
    }

    pub const fn exit(self) -> u8 {
        match self {
            Self::EUsage | Self::EUnsupported | Self::EShareLink => 2,
            Self::EAuth | Self::EConfig => 3,
            Self::ENotFound => 4,
            Self::ERateLimited => 5,
            Self::ENetwork => 6,
            Self::EUpstream => 7,
            Self::EOfflineMiss => 8,
            Self::EStoreSchema => 9,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub http_status: Option<u16>,
    pub retry_after: Option<u64>,
    pub hint: Option<String>,
    pub suggested_commands: Vec<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            http_status: None,
            retry_after: None,
            hint: None,
            suggested_commands: Vec::new(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::EUsage, message)
    }

    pub fn upstream(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::EUpstream, message)
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}
