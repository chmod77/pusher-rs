/// A client for interacting with the Pusher service.
///
mod auth;
mod channels;
mod config;
mod error;
mod events;
mod websocket;

use aes::{
    cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit},
    Aes256,
};
use cbc::{Decryptor, Encryptor};
use hmac::{Hmac, Mac};
use log::{debug, error, info};
use rand::Rng;
use serde_json::json;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use url::Url;

pub use auth::PusherAuth;
pub use channels::{Channel, ChannelType};
pub use config::PusherConfig;
pub use error::{PusherError, PusherResult};
pub use events::{Event, SystemEvent};

use websocket::WebSocketClient;

/// This struct provides methods for connecting to Pusher, subscribing to channels,
/// triggering events, and handling incoming events.
pub struct PusherClient {
    config: PusherConfig,
    auth: PusherAuth,
    websocket: Option<WebSocketClient>,
    channels: Arc<RwLock<HashMap<String, Channel>>>,
    event_handlers: Arc<RwLock<HashMap<String, Vec<Box<dyn Fn(Event) + Send + Sync + 'static>>>>>,
    state: Arc<RwLock<ConnectionState>>,
    event_tx: mpsc::Sender<Event>,
    encrypted_channels: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

impl PusherClient {
    /// Creates a new `PusherClient` instance with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - The configuration for the Pusher client.
    ///
    /// # Returns
    ///
    /// A `PusherResult` containing the new `PusherClient` instance.
    pub fn new(config: PusherConfig) -> PusherResult<Self> {
        let auth = PusherAuth::new(&config.app_key, &config.app_secret);
        let (event_tx, event_rx) = mpsc::channel(100);
        let state = Arc::new(RwLock::new(ConnectionState::Disconnected));
        let event_handlers = Arc::new(RwLock::new(HashMap::new()));
        let encrypted_channels = Arc::new(RwLock::new(HashMap::new()));

        let client = Self {
            config,
            auth,
            websocket: None,
            channels: Arc::new(RwLock::new(HashMap::new())),
            event_handlers: event_handlers.clone(),
            state: state.clone(),
            event_tx,
            encrypted_channels,
        };

        // Spawn the event handling task
        tokio::spawn(Self::handle_events(event_rx, event_handlers));

        Ok(client)
    }
    async fn handle_events(
        mut event_rx: mpsc::Receiver<Event>,
        event_handlers: Arc<
            RwLock<HashMap<String, Vec<Box<dyn Fn(Event) + Send + Sync + 'static>>>>,
        >,
    ) {
        while let Some(event) = event_rx.recv().await {
            let handlers = event_handlers.read().await;
            if let Some(callbacks) = handlers.get(&event.event) {
                for callback in callbacks {
                    callback(event.clone());
                }
            }
        }
    }

    /// Connects to the Pusher server.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn connect(&mut self) -> PusherResult<()> {
        let url = self.get_websocket_url()?;
        let mut websocket =
            WebSocketClient::new(url.clone(), Arc::clone(&self.state), self.event_tx.clone());
        log::info!("Connecting to Pusher using URL: {}", url);
        websocket.connect().await?;
        self.websocket = Some(websocket);

        // Start the WebSocket event loop
        let mut ws = self.websocket.take().unwrap();
        tokio::spawn(async move {
            ws.run().await;
        });

        Ok(())
    }

    /// Disconnects from the Pusher server.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn disconnect(&mut self) -> PusherResult<()> {
        if let Some(websocket) = &self.websocket {
            websocket.close().await?;
        }
        *self.state.write().await = ConnectionState::Disconnected;
        self.websocket = None;
        Ok(())
    }

    /// Subscribes to a channel.
    ///
    /// # Arguments
    ///
    /// * `channel_name` - The name of the channel to subscribe to.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn subscribe(&mut self, channel_name: &str) -> PusherResult<()> {
        let channel = Channel::new(channel_name);
        let mut channels = self.channels.write().await;
        channels.insert(channel_name.to_string(), channel);

        if let Some(websocket) = &self.websocket {
            let data = json!({
                "event": "pusher:subscribe",
                "data": {
                    "channel": channel_name
                }
            });
            websocket.send(serde_json::to_string(&data)?).await?;
        } else {
            return Err(PusherError::ConnectionError("Not connected".into()));
        }

        Ok(())
    }

    /// Subscribes to an encrypted channel.
    ///
    /// # Arguments
    ///
    /// * `channel_name` - The name of the encrypted channel to subscribe to.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn subscribe_encrypted(&mut self, channel_name: &str) -> PusherResult<()> {
        if !channel_name.starts_with("private-encrypted-") {
            return Err(PusherError::ChannelError(
                "Encrypted channels must start with 'private-encrypted-'".to_string(),
            ));
        }

        let shared_secret = self.generate_shared_secret(channel_name);

        {
            let mut encrypted_channels = self.encrypted_channels.write().await;
            encrypted_channels.insert(channel_name.to_string(), shared_secret);
        }

        self.subscribe(channel_name).await
    }

    /// Unsubscribes from a channel.
    ///
    /// # Arguments
    ///
    /// * `channel_name` - The name of the channel to unsubscribe from.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn unsubscribe(&mut self, channel_name: &str) -> PusherResult<()> {
        {
            let mut channels = self.channels.write().await;
            channels.remove(channel_name);
        }

        {
            let mut encrypted_channels = self.encrypted_channels.write().await;
            encrypted_channels.remove(channel_name);
        }

        if let Some(websocket) = &self.websocket {
            let data = json!({
                "event": "pusher:unsubscribe",
                "data": {
                    "channel": channel_name
                }
            });
            websocket.send(serde_json::to_string(&data)?).await?;
        } else {
            return Err(PusherError::ConnectionError("Not connected".into()));
        }

        Ok(())
    }

    /// Triggers an event on a channel.
    ///
    /// # Arguments
    ///
    /// * `channel` - The name of the channel to trigger the event on.
    /// * `event` - The name of the event to trigger.
    /// * `data` - The data to send with the event.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn trigger(&self, channel: &str, event: &str, data: &str) -> PusherResult<()> {
        let url = format!(
            "https://api-{}.pusher.com/apps/{}/events",
            self.config.cluster, self.config.app_id
        );

        let body = json!({
            "name": event,
            "channel": channel,
            "data": data
        });
        let path = format!("/apps/{}/events", self.config.app_id);
        let auth_params = self.auth.authenticate_request("POST", &path, &body)?;

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .json(&body)
            .query(&auth_params)
            .send()
            .await?;
        let response_status = response.status();
        if response_status.is_success() {
            Ok(())
        } else {
            let error_body = response.text().await?;
            Err(PusherError::ApiError(format!(
                "Failed to trigger event: {} - {}",
                response_status, error_body
            )))
        }
    }

    /// Triggers an event on an encrypted channel.
    ///
    /// # Arguments
    ///
    /// * `channel` - The name of the encrypted channel to trigger the event on.
    /// * `event` - The name of the event to trigger.
    /// * `data` - The data to send with the event.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn trigger_encrypted(
        &self,
        channel: &str,
        event: &str,
        data: &str,
    ) -> PusherResult<()> {
        let shared_secret = {
            let encrypted_channels = self.encrypted_channels.read().await;
            encrypted_channels
                .get(channel)
                .ok_or_else(|| {
                    PusherError::ChannelError(
                        "Channel is not subscribed or is not encrypted".to_string(),
                    )
                })?
                .clone()
        };

        let encrypted_data = self.encrypt_data(data, &shared_secret)?;
        self.trigger(channel, event, &encrypted_data).await
    }

    /// Binds a callback to an event.
    ///
    /// # Arguments
    ///
    /// * `event_name` - The name of the event to bind to.
    /// * `callback` - The callback function to execute when the event occurs.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    ///
    /// # Example
    ///
    /// ```
    /// use pusher_rs::{PusherClient, PusherConfig, PusherResult, Event};
    /// use tokio;
    ///
    /// #[tokio::main]
    /// async fn main() -> PusherResult<()> {
    ///     let config = PusherConfig::from_env()?;
    ///     let client = PusherClient::new(config)?;
    ///     client.connect().await?;
    ///     
    ///     client.bind("test-event", |event: Event| {
    ///         println!("Received event: {:?}", event);
    ///     }).await?;
    ///     
    ///     // Keep the connection alive for a while
    ///     tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    ///     
    ///     Ok(())
    /// }
    /// ```
    pub async fn bind<F>(&self, event_name: &str, callback: F) -> PusherResult<()>
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        let mut handlers = self.event_handlers.write().await;
        handlers
            .entry(event_name.to_string())
            .or_insert_with(Vec::new)
            .push(Box::new(callback));
        Ok(())
    }

    async fn handle_event(
        event: Event,
        handlers: &Arc<RwLock<HashMap<String, Vec<Box<dyn Fn(Event) + Send + Sync + 'static>>>>>,
    ) -> PusherResult<()> {
        let handlers = handlers.read().await;
        if let Some(callbacks) = handlers.get(&event.event) {
            for callback in callbacks {
                callback(event.clone());
            }
        }
        Ok(())
    }

    fn get_websocket_url(&self) -> PusherResult<Url> {
        let scheme = if self.config.use_tls { "wss" } else { "ws" };
        info!("Connecting to Pusher using scheme: {}", scheme);

        let default_host = format!("ws-{}.pusher.com", self.config.cluster);
        let host = self.config.host.as_deref().unwrap_or(&default_host);

        let url = format!(
            "{}://{}/app/{}?protocol=7",
            scheme, host, self.config.app_key
        );

        info!("WebSocket URL: {}", url);
        Url::parse(&url).map_err(PusherError::from)
    }

    fn generate_shared_secret(&self, channel_name: &str) -> Vec<u8> {
        let mut hmac = Hmac::<Sha256>::new_from_slice(self.config.app_secret.as_bytes())
            .expect("HMAC can take key of any size");
        hmac.update(channel_name.as_bytes());
        hmac.finalize().into_bytes().to_vec()
    }

    fn encrypt_data(&self, data: &str, shared_secret: &[u8]) -> PusherResult<String> {
        let iv = rand::thread_rng().gen::<[u8; 16]>();
        let cipher = Encryptor::<Aes256>::new(shared_secret.into(), &iv.into());

        let plaintext = data.as_bytes();
        let mut buffer = vec![0u8; plaintext.len() + 16]; // Add space for padding. 16 is the block size. We shall revisit.
        buffer[..plaintext.len()].copy_from_slice(plaintext);

        let ciphertext_len = cipher
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, plaintext.len())
            .map_err(|e| PusherError::EncryptionError(e.to_string()))?
            .len();

        let mut result = iv.to_vec();
        result.extend_from_slice(&buffer[..ciphertext_len]);

        Ok(base64::encode(result))
    }

    /// Decrypts encrypted data using the shared secret.
    ///
    /// # Arguments
    ///
    /// * `encrypted_data` - The encrypted data to decrypt.
    /// * `shared_secret` - The shared secret to use for decryption.
    ///
    /// # Returns
    ///
    /// A `PusherResult` containing the decrypted data.
    ///
    /// # Errors
    ///
    /// Returns a `PusherError` if the data cannot be decrypted.
    ///
    /// # Example
    ///
    /// ```
    /// # use pusher::PusherClient;
    /// # use pusher::PusherConfig;
    /// # use pusher::PusherResult;
    /// # use pusher::ConnectionState;
    /// # use std::sync::Arc;
    /// # use tokio::sync::RwLock;
    /// # use std::env;
    /// # async fn decrypt_data() -> PusherResult<()> {
    /// let config = PusherConfig::from_env()?;
    /// let client = PusherClient::new(config)?;
    /// let shared_secret = client.generate
    /// let encrypted_data = "encrypted_data";
    /// let decrypted_data = client.decrypt_data(encrypted_data, shared_secret)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    fn decrypt_data(&self, encrypted_data: &str, shared_secret: &[u8]) -> PusherResult<String> {
        let decoded = base64::decode(encrypted_data)
            .map_err(|e| PusherError::DecryptionError(e.to_string()))?;

        if decoded.len() < 16 {
            return Err(PusherError::DecryptionError(
                "Invalid encrypted data".to_string(),
            ));
        }

        let (iv, ciphertext) = decoded.split_at(16);
        let cipher = Decryptor::<Aes256>::new(shared_secret.into(), iv.into());

        let mut buffer = ciphertext.to_vec();
        let decrypted_data = cipher
            .decrypt_padded_mut::<Pkcs7>(&mut buffer)
            .map_err(|e| PusherError::DecryptionError(e.to_string()))?;

        String::from_utf8(decrypted_data.to_vec())
            .map_err(|e| PusherError::DecryptionError(e.to_string()))
    }

    /// Gets the current connection state.
    ///
    /// # Returns
    ///
    /// The current `ConnectionState`.
    pub async fn get_connection_state(&self) -> ConnectionState {
        self.state.read().await.clone()
    }

    /// Gets a list of currently subscribed channels.
    ///
    /// # Returns
    ///
    /// A vector of channel names.
    pub async fn get_subscribed_channels(&self) -> Vec<String> {
        self.channels.read().await.keys().cloned().collect()
    }

    /// Sends a test event through the client.
    ///
    /// # Arguments
    ///
    /// * `event` - The event to send.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn send_test_event(&self, event: Event) -> PusherResult<()> {
        self.event_tx
            .send(event)
            .await
            .map_err(|e| PusherError::WebSocketError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let config =
            PusherConfig::from_env().expect("Failed to load Pusher configuration from environment");
        let client = PusherClient::new(config).unwrap();
        assert_eq!(*client.state.read().await, ConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn test_generate_shared_secret() {
        let config =
            PusherConfig::from_env().expect("Failed to load Pusher configuration from environment");
        let client = PusherClient::new(config).unwrap();
        let secret = client.generate_shared_secret("test-channel");
        assert!(!secret.is_empty());
    }
}
