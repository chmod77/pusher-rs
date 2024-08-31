use dotenv::dotenv;
use std::env;
use std::time::Duration;

/// Config for the Pusher client. We are defaulting to .env. 
/// TODO: look into .toml or .yaml
#[derive(Clone, Debug)]
pub struct PusherConfig {
    /// The Pusher App ID.
    pub app_id: String,

    /// The Pusher App key.
    pub app_key: String,

    /// The Pusher App secret.
    pub app_secret: String,

    /// The cluster.
    pub cluster: String,

    /// Whether to use TLS for connections. Defaults to true.
    pub use_tls: bool,

    /// The host to connect to. If None, the default Pusher host will be used.
    pub host: Option<String>,

    /// The maximum number of reconnection attempts. Defaults to 6.
    pub max_reconnection_attempts: u32,

    /// The backoff interval for reconnection attempts. Defaults to 1 second.
    pub backoff_interval: Duration,

    /// The activity timeout. Defaults to 120 seconds.
    pub activity_timeout: Duration,

    /// The pong timeout. Defaults to 30 seconds.
    pub pong_timeout: Duration,
}

impl Default for PusherConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            app_key: String::new(),
            app_secret: String::new(),
            cluster: String::new(),
            use_tls: false,
            host: None,
            max_reconnection_attempts: 6,
            backoff_interval: Duration::from_secs(1),
            activity_timeout: Duration::from_secs(120),
            pong_timeout: Duration::from_secs(30),
        }
    }
}

impl PusherConfig {
    pub fn from_env() -> Result<Self, env::VarError> {
        dotenv().ok(); // This line loads the .env file
        let cluster = env::var("PUSHER_CLUSTER").unwrap_or_else(|_| "mt1".to_string()); //Default to mt1.
        let host = env::var("PUSHER_HOST")
            .ok()
            .unwrap_or_else(|| format!("ws-{}.pusher.com", cluster)); // let's read <> build the host from the env variable

        Ok(Self {
            app_id: env::var("PUSHER_APP_ID")?,
            app_key: env::var("PUSHER_KEY")?,
            app_secret: env::var("PUSHER_SECRET")?,
            cluster,
            use_tls: env::var("PUSHER_USE_TLS")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(true),
            host: Some(host),
            max_reconnection_attempts: env::var("PUSHER_MAX_RECONNECTION_ATTEMPTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(6),
            backoff_interval: Duration::from_secs(
                env::var("PUSHER_BACKOFF_INTERVAL")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
            ),
            activity_timeout: Duration::from_secs(
                env::var("PUSHER_ACTIVITY_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(120),
            ),
            pong_timeout: Duration::from_secs(
                env::var("PUSHER_PONG_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PusherConfig::default();
        // assert!(config.use_tls);
        assert_eq!(config.max_reconnection_attempts, 6);
        assert_eq!(config.backoff_interval, Duration::from_secs(1));
        assert_eq!(config.activity_timeout, Duration::from_secs(120));
        assert_eq!(config.pong_timeout, Duration::from_secs(30));
    }

    #[test]
    #[ignore]
    fn test_new_config() {
        let config =
            PusherConfig::from_env().expect("Failed to load Pusher configuration from environment");
        assert_eq!(config.app_id, "app_id");
        assert_eq!(config.app_key, "app_key");
        assert_eq!(config.app_secret, "app_secret");
        assert_eq!(config.cluster, "eu");
    }
}
