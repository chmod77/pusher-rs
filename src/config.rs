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

/// Builder for `PusherConfig` to enable programmatic configuration.
///
/// # Example
/// ```rust,no_run
/// use pusher_rs::PusherConfig;
/// use std::time::Duration;
///
/// let config = PusherConfig::builder()
///     .app_id("your_app_id")
///     .app_key("your_app_key")
///     .app_secret("your_app_secret")
///     .cluster("eu")
///     .use_tls(true)
///     .max_reconnection_attempts(10)
///     .backoff_interval(Duration::from_secs(2))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct PusherConfigBuilder {
    app_id: Option<String>,
    app_key: Option<String>,
    app_secret: Option<String>,
    cluster: Option<String>,
    use_tls: bool,
    host: Option<String>,
    max_reconnection_attempts: u32,
    backoff_interval: Duration,
    activity_timeout: Duration,
    pong_timeout: Duration,
}

impl Default for PusherConfigBuilder {
    fn default() -> Self {
        Self {
            app_id: None,
            app_key: None,
            app_secret: None,
            cluster: None,
            use_tls: true,
            host: None,
            max_reconnection_attempts: 6,
            backoff_interval: Duration::from_secs(1),
            activity_timeout: Duration::from_secs(120),
            pong_timeout: Duration::from_secs(30),
        }
    }
}

impl PusherConfigBuilder {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Pusher App ID.
    pub fn app_id<S: Into<String>>(mut self, app_id: S) -> Self {
        self.app_id = Some(app_id.into());
        self
    }

    /// Sets the Pusher App key.
    pub fn app_key<S: Into<String>>(mut self, app_key: S) -> Self {
        self.app_key = Some(app_key.into());
        self
    }

    /// Sets the Pusher App secret.
    pub fn app_secret<S: Into<String>>(mut self, app_secret: S) -> Self {
        self.app_secret = Some(app_secret.into());
        self
    }

    /// Sets the cluster.
    pub fn cluster<S: Into<String>>(mut self, cluster: S) -> Self {
        self.cluster = Some(cluster.into());
        self
    }

    /// Sets whether to use TLS for connections.
    pub fn use_tls(mut self, use_tls: bool) -> Self {
        self.use_tls = use_tls;
        self
    }

    /// Sets a custom host to connect to.
    pub fn host<S: Into<String>>(mut self, host: S) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Sets the maximum number of reconnection attempts.
    pub fn max_reconnection_attempts(mut self, max_reconnection_attempts: u32) -> Self {
        self.max_reconnection_attempts = max_reconnection_attempts;
        self
    }

    /// Sets the backoff interval for reconnection attempts.
    pub fn backoff_interval(mut self, backoff_interval: Duration) -> Self {
        self.backoff_interval = backoff_interval;
        self
    }

    /// Sets the activity timeout.
    pub fn activity_timeout(mut self, activity_timeout: Duration) -> Self {
        self.activity_timeout = activity_timeout;
        self
    }

    /// Sets the pong timeout.
    pub fn pong_timeout(mut self, pong_timeout: Duration) -> Self {
        self.pong_timeout = pong_timeout;
        self
    }

    /// Builds the `PusherConfig` from the builder.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields (app_id, app_key, app_secret) are missing.
    pub fn build(self) -> Result<PusherConfig, String> {
        let app_id = self.app_id.ok_or("app_id is required")?;
        let app_key = self.app_key.ok_or("app_key is required")?;
        let app_secret = self.app_secret.ok_or("app_secret is required")?;
        let cluster = self.cluster.unwrap_or_else(|| "mt1".to_string());
        let host = self
            .host
            .or_else(|| Some(format!("ws-{}.pusher.com", cluster)));

        Ok(PusherConfig {
            app_id,
            app_key,
            app_secret,
            cluster,
            use_tls: self.use_tls,
            host,
            max_reconnection_attempts: self.max_reconnection_attempts,
            backoff_interval: self.backoff_interval,
            activity_timeout: self.activity_timeout,
            pong_timeout: self.pong_timeout,
        })
    }
}

impl PusherConfig {
    /// Creates a new builder for `PusherConfig`.
    pub fn builder() -> PusherConfigBuilder {
        PusherConfigBuilder::new()
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
