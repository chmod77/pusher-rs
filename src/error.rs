use thiserror::Error;
use std::io;
use url::ParseError;
use reqwest;
use serde_json;

/// Specific errors that can occur when interacting with the Pusher API.
/// TODO: Add more specific errors
#[derive(Error, Debug)]
pub enum PusherError {
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),

    #[error("URL parse error: {0}")]
    UrlParseError(#[from] ParseError),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Event error: {0}")]
    EventError(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Rate limit error: {0}")]
    RateLimitError(String),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("Presence data error: {0}")]
    PresenceDataError(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Timeout error: {0}")]
    TimeoutError(String),

    #[error("Unknown error: {0}")]
    UnknownError(String),
}

impl From<String> for PusherError {
    fn from(error: String) -> Self {
        PusherError::UnknownError(error)
    }
}

impl From<&str> for PusherError {
    fn from(error: &str) -> Self {
        PusherError::UnknownError(error.to_string())
    }
}

/// A specialized Result type for Pusher operations.
pub type PusherResult<T> = Result<T, PusherError>;

// Helper functions for creating specific errors

pub fn auth_error(message: impl Into<String>) -> PusherError {
    PusherError::AuthError(message.into())
}

pub fn channel_error(message: impl Into<String>) -> PusherError {
    PusherError::ChannelError(message.into())
}

pub fn event_error(message: impl Into<String>) -> PusherError {
    PusherError::EventError(message.into())
}

pub fn connection_error(message: impl Into<String>) -> PusherError {
    PusherError::ConnectionError(message.into())
}

pub fn config_error(message: impl Into<String>) -> PusherError {
    PusherError::ConfigError(message.into())
}

pub fn rate_limit_error(message: impl Into<String>) -> PusherError {
    PusherError::RateLimitError(message.into())
}

pub fn encryption_error(message: impl Into<String>) -> PusherError {
    PusherError::EncryptionError(message.into())
}

pub fn decryption_error(message: impl Into<String>) -> PusherError {
    PusherError::DecryptionError(message.into())
}

pub fn presence_data_error(message: impl Into<String>) -> PusherError {
    PusherError::PresenceDataError(message.into())
}

pub fn api_error(message: impl Into<String>) -> PusherError {
    PusherError::ApiError(message.into())
}

pub fn timeout_error(message: impl Into<String>) -> PusherError {
    PusherError::TimeoutError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_conversion() {
        let err: PusherError = "Test error".into();
        assert!(matches!(err, PusherError::UnknownError(_)));

        let err: PusherError = String::from("Test error").into();
        assert!(matches!(err, PusherError::UnknownError(_)));
    }

    #[test]
    fn test_error_display() {
        let err = PusherError::AuthError("Invalid credentials".to_string());
        assert_eq!(err.to_string(), "Authentication error: Invalid credentials");

        let err = PusherError::ChannelError("Channel not found".to_string());
        assert_eq!(err.to_string(), "Channel error: Channel not found");
    }

    #[test]
    fn test_helper_functions() {
        let err = auth_error("Invalid token");
        assert!(matches!(err, PusherError::AuthError(_)));

        let err = channel_error("Invalid channel name");
        assert!(matches!(err, PusherError::ChannelError(_)));

        let err = event_error("Event too large");
        assert!(matches!(err, PusherError::EventError(_)));
    }
}