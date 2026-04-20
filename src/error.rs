use std::path::PathBuf;

use thiserror::Error;

use crate::config::ServerId;

#[derive(Debug, Error)]
pub enum CmcpError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("invalid mcp uri: {0}")]
    InvalidUri(String),

    #[error("invalid tool argument '{arg}': {reason}")]
    InvalidArg { arg: String, reason: String },

    #[error("server '{0}' not found in any known config source")]
    ServerNotFound(ServerId),

    #[error("tool '{tool}' not found on server '{server}'")]
    ToolNotFound { server: ServerId, tool: String },

    #[error("transport error: {0}")]
    Transport(String),

    #[error("mcp service error: {0}")]
    Service(String),

    #[error("tool call timed out after {0}s")]
    Timeout(u64),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("home directory not found")]
    NoHomeDir,
}

impl CmcpError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CmcpError::Config(_) | CmcpError::InvalidUri(_) | CmcpError::InvalidArg { .. } => 2,
            CmcpError::ServerNotFound(_) | CmcpError::Transport(_) => 3,
            CmcpError::ToolNotFound { .. } | CmcpError::Service(_) => 4,
            CmcpError::Timeout(_) => 5,
            CmcpError::Io(_) | CmcpError::Json(_) => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, CmcpError>;
