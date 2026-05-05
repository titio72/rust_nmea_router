use std::fmt;

/// Top-level typed error for the three main failure domains exposed at public
/// API boundaries (initialization, config, DB, web server). Internal operations
/// continue using `Result<T, Box<dyn Error>>` per CLAUDE.md conventions.
#[derive(Debug)]
pub enum AppError {
    Database(String),
    Io(String),
    Configuration(String),
    Parse(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Database(msg) => write!(f, "Database error: {}", msg),
            AppError::Io(msg) => write!(f, "IO error: {}", msg),
            AppError::Configuration(msg) => write!(f, "Configuration error: {}", msg),
            AppError::Parse(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl AppError {
    /// Static category label for structured logging.
    #[allow(dead_code)]
    pub fn category(&self) -> &'static str {
        match self {
            AppError::Database(_) => "database",
            AppError::Io(_) => "io",
            AppError::Configuration(_) => "configuration",
            AppError::Parse(_) => "parse",
        }
    }
}

impl From<mysql::Error> for AppError {
    fn from(e: mysql::Error) -> Self {
        AppError::Database(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Configuration(e.to_string())
    }
}

impl From<chrono::ParseError> for AppError {
    fn from(e: chrono::ParseError) -> Self {
        AppError::Parse(e.to_string())
    }
}

impl From<std::str::Utf8Error> for AppError {
    fn from(e: std::str::Utf8Error) -> Self {
        AppError::Parse(e.to_string())
    }
}
