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
use base64::{engine::general_purpose, Engine as _};
use cbc::{Decryptor, Encryptor};
use hmac::{Hmac, Mac};
use log::info;
use rand::Rng;
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use url::Url;

pub use auth::PusherAuth;
pub use channels::{Channel, ChannelList, ChannelType};
pub use config::PusherConfig;
pub use error::{PusherError, PusherResult};
pub use events::{Event, SystemEvent, SystemEventData};

use websocket::{WebSocketClient, WebSocketCommand};

/// This struct provides methods for connecting to Pusher, subscribing to channels,
/// triggering events, and handling incoming events.
pub struct PusherClient {
    config: PusherConfig,
    auth: PusherAuth,
    // websocket: Option<WebSocketClient>,
    websocket_command_tx: Arc<RwLock<Option<mpsc::Sender<WebSocketCommand>>>>,
    channels: Arc<RwLock<HashMap<String, Channel>>>,
    event_handlers: Arc<RwLock<HashMap<String, Vec<Box<dyn Fn(Event) + Send + Sync + 'static>>>>>,
    state: Arc<RwLock<ConnectionState>>,
    event_tx: mpsc::Sender<Event>,
    event_broadcast: broadcast::Sender<Event>,
    encrypted_channels: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    socket_id: Arc<RwLock<Option<String>>>,
}

#[derive(Debug, Clone)]
pub struct BatchEvent {
    pub channel: String,
    pub event: String,
    pub data: String,
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
        let (event_broadcast, _) = broadcast::channel(100);
        let state = Arc::new(RwLock::new(ConnectionState::Disconnected));
        let event_handlers = Arc::new(RwLock::new(std::collections::HashMap::new()));
        let encrypted_channels = Arc::new(RwLock::new(std::collections::HashMap::new()));

        let socket_id = Arc::new(RwLock::new(None));
        let socket_id_for_handler = Arc::clone(&socket_id);
        let event_broadcast_for_handler = event_broadcast.clone();

        let client = Self {
            config,
            auth,
            websocket_command_tx: Arc::new(RwLock::new(None)),
            channels: Arc::new(RwLock::new(std::collections::HashMap::new())),
            event_handlers: event_handlers.clone(),
            state: state.clone(),
            event_tx,
            event_broadcast,
            encrypted_channels,
            socket_id,
        };

        tokio::spawn(Self::handle_events(
            event_rx,
            event_handlers,
            socket_id_for_handler,
            event_broadcast_for_handler,
        ));

        Ok(client)
    }

    async fn send(&self, message: String) -> PusherResult<()> {
        let websocket_command_tx = self.websocket_command_tx.read().await;
        if let Some(tx) = websocket_command_tx.as_ref() {
            tx.send(WebSocketCommand::Send(message))
                .await
                .map_err(|e| {
                    PusherError::WebSocketError(format!("Failed to send command: {}", e))
                })?;
            Ok(())
        } else {
            Err(PusherError::ConnectionError("Not connected".into()))
        }
    }

    async fn handle_events(
        mut event_rx: mpsc::Receiver<Event>,
        event_handlers: Arc<
            RwLock<
                std::collections::HashMap<String, Vec<Box<dyn Fn(Event) + Send + Sync + 'static>>>,
            >,
        >,
        socket_id: Arc<RwLock<Option<String>>>,
        event_broadcast: broadcast::Sender<Event>,
    ) {
        while let Some(event) = event_rx.recv().await {
            // Broadcast to all stream subscribers (zero-copy, ignores if no subscribers)
            let _ = event_broadcast.send(event.clone());

            // Extract socket_id from connection_established events
            if event.event == "pusher:connection_established" {
                if let Some(system_event) = event.as_system_event() {
                    if let SystemEventData::ConnectionEstablished { socket_id: sid, .. } =
                        system_event.data
                    {
                        let mut socket_id_guard = socket_id.write().await;
                        *socket_id_guard = Some(sid);
                    }
                }
            }

            // Call registered callbacks
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
    pub async fn connect(&self) -> PusherResult<()> {
        let url = self.get_websocket_url()?;
        let (command_tx, command_rx) = mpsc::channel(100);

        let mut websocket = WebSocketClient::new(
            url.clone(),
            Arc::clone(&self.state),
            self.event_tx.clone(),
            command_rx,
        );

        log::info!("Connecting to Pusher using URL: {}", url);
        websocket.connect().await?;

        tokio::spawn(async move {
            websocket.run().await;
        });

        *self.websocket_command_tx.write().await = Some(command_tx);

        Ok(())
    }

    /// Disconnects from the Pusher server.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn disconnect(&self) -> PusherResult<()> {
        let mut websocket_command_tx = self.websocket_command_tx.write().await;
        if let Some(tx) = websocket_command_tx.take() {
            tx.send(WebSocketCommand::Close).await.map_err(|e| {
                PusherError::WebSocketError(format!("Failed to send close command: {}", e))
            })?;
        }
        *self.state.write().await = ConnectionState::Disconnected;
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
    pub async fn subscribe<S: AsRef<str>>(&self, channel_name: S) -> PusherResult<()> {
        let channel_name = channel_name.as_ref();
        let channel = Channel::new(channel_name);
        let mut channels = self.channels.write().await;
        channels.insert(channel_name.to_string(), channel);

        let data = json!({
            "event": "pusher:subscribe",
            "data": {
                "channel": channel_name
            }
        });

        self.send(serde_json::to_string(&data)?).await
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
    pub async fn subscribe_encrypted<S: AsRef<str>>(&self, channel_name: S) -> PusherResult<()> {
        let channel_name = channel_name.as_ref();
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

    /// Gets the current socket ID.
    ///
    /// # Returns
    ///
    /// A `PusherResult` containing the socket ID if connected, or an error if not connected.
    pub async fn get_socket_id(&self) -> PusherResult<String> {
        let socket_id_guard = self.socket_id.read().await;
        socket_id_guard.clone().ok_or_else(|| {
            PusherError::ConnectionError("Not connected or socket ID not available".into())
        })
    }

    /// Authenticates a presence channel subscription.
    ///
    /// # Arguments
    ///
    /// * `socket_id` - The socket ID from the connection.
    /// * `channel_name` - The name of the presence channel.
    /// * `user_id` - The user ID for the presence channel.
    /// * `user_info` - Optional user information as a JSON value.
    ///
    /// # Returns
    ///
    /// A `PusherResult` containing the authentication string.
    pub fn authenticate_presence_channel(
        &self,
        socket_id: &str,
        channel_name: &str,
        user_id: &str,
        user_info: Option<&Value>,
    ) -> PusherResult<String> {
        self.auth
            .authenticate_presence_channel(socket_id, channel_name, user_id, user_info)
    }

    /// Subscribes to a channel with authentication data.
    ///
    /// # Arguments
    ///
    /// * `channel_name` - The name of the channel to subscribe to.
    /// * `auth` - The authentication string (from `authenticate_presence_channel` or similar).
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn subscribe_with_auth<S1: AsRef<str>, S2: AsRef<str>>(
        &self,
        channel_name: S1,
        auth: S2,
    ) -> PusherResult<()> {
        let channel_name = channel_name.as_ref();
        let auth = auth.as_ref();
        let channel = Channel::new(channel_name);
        let mut channels = self.channels.write().await;
        channels.insert(channel_name.to_string(), channel);

        let data = json!({
            "event": "pusher:subscribe",
            "data": {
                "channel": channel_name,
                "auth": auth
            }
        });

        self.send(serde_json::to_string(&data)?).await
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
    ///
    pub async fn unsubscribe<S: AsRef<str>>(&self, channel_name: S) -> PusherResult<()> {
        let channel_name = channel_name.as_ref();
        {
            let mut channels = self.channels.write().await;
            channels.remove(channel_name);
        }

        {
            let mut encrypted_channels = self.encrypted_channels.write().await;
            encrypted_channels.remove(channel_name);
        }

        let data = json!({
            "event": "pusher:unsubscribe",
            "data": {
                "channel": channel_name
            }
        });

        self.send(serde_json::to_string(&data)?).await
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
    pub async fn trigger<S1: AsRef<str>, S2: AsRef<str>, S3: AsRef<str>>(
        &self,
        channel: S1,
        event: S2,
        data: S3,
    ) -> PusherResult<()> {
        let channel = channel.as_ref();
        let event = event.as_ref();
        let data = data.as_ref();
        let url = format!(
            "https://api-{}.pusher.com/apps/{}/events",
            self.config.cluster, self.config.app_id
        );

        // Validate that the data is valid JSON, but keep it as a string
        serde_json::from_str::<serde_json::Value>(data).map_err(|e| PusherError::JsonError(e))?;

        let body = json!({
            "name": event,
            "channel": channel,
            "data": data, // Keep data as a string
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
    pub async fn trigger_encrypted<S1: AsRef<str>, S2: AsRef<str>, S3: AsRef<str>>(
        &self,
        channel: S1,
        event: S2,
        data: S3,
    ) -> PusherResult<()> {
        let channel = channel.as_ref();
        let event = event.as_ref();
        let data = data.as_ref();
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

    /// Triggers multiple events in a single API call.
    ///
    /// # Arguments
    ///
    /// * `batch_events` - An iterator of `BatchEvent` structs, each containing channel, event, and data.
    ///
    /// # Returns
    ///
    /// A `PusherResult` indicating success or failure.
    pub async fn trigger_batch<I>(&self, batch_events: I) -> PusherResult<()>
    where
        I: IntoIterator<Item = BatchEvent>,
    {
        let url = format!(
            "https://api-{}.pusher.com/apps/{}/batch_events",
            self.config.cluster, self.config.app_id
        );

        let events: Vec<serde_json::Value> = batch_events
            .into_iter()
            .map(|event| {
                json!({
                    "channel": event.channel,
                    "name": event.event,
                    "data": event.data
                })
            })
            .collect();

        let body = json!({ "batch": events });
        let path = format!("/apps/{}/batch_events", self.config.app_id);
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
                "Failed to trigger batch events: {} - {}",
                response_status, error_body
            )))
        }
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

        Ok(general_purpose::STANDARD.encode(result))
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
    /// Decrypts encrypted data using the shared secret.
    ///
    /// This method is useful for decrypting data received from encrypted channels.
    pub fn decrypt_data(&self, encrypted_data: &str, shared_secret: &[u8]) -> PusherResult<String> {
        let decoded = general_purpose::STANDARD
            .decode(encrypted_data)
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

    /// Gets the channel Occupancy
    ///
    /// # Returns
    ///
    /// A PusherResult  
    pub async fn get_channel_occupancy<S: AsRef<str>>(&self, channel_name: S) -> PusherResult<u32> {
        let channel_name = channel_name.as_ref();
        let path = format!("/apps/{}/channels/{}", self.config.app_id, channel_name);
        let url = format!("https://api-{}.pusher.com{}", self.config.cluster, path);

        // Create an empty body for GET request
        let body = serde_json::json!({});
        log::info!("BODY: {:?}", body);
        log::info!("URL {:?}", url);
        let mut params = self.auth.authenticate_request("GET", &path, &body)?;

        log::info!("PARAMS: {:?}", params);
        // Add the info parameter to the query string, not the body
        params.insert("info".to_string(), "subscription_count".to_string());

        let client = reqwest::Client::new();
        let response = client.get(&url).query(&params).send().await?;
        log::info!("URL: {:?}", response.url());
        let status = response.status();

        if status.is_success() {
            let body: serde_json::Value = response.json().await?;
            Ok(body
                .get("occupied")
                .and_then(|v| v.as_bool())
                .map(|occupied| if occupied { 1 } else { 0 })
                .or_else(|| {
                    body["subscription_count"]
                        .as_u64()
                        .map(|count| count as u32)
                })
                .unwrap_or(0))
        } else {
            let error_body = response.text().await?;
            Err(PusherError::ApiError(format!(
                "Failed to get channel occupancy: {} - {}",
                status, error_body
            )))
        }
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

    /// Subscribes to all events, returning a `broadcast::Receiver` that implements `Stream`.
    ///
    /// This is the most idiomatic Rust way to handle events. The receiver can be used with
    /// `futures_util::StreamExt` combinators like `filter`, `map`, `take`, etc.
    ///
    /// # Example
    /// ```rust,no_run
    /// use futures_util::StreamExt;
    ///
    /// let mut events = client.subscribe_events();
    ///
    /// // Process events in a loop
    /// while let Ok(event) = events.recv().await {
    ///     println!("Received: {:?}", event);
    /// }
    ///
    /// // Or use Stream combinators
    /// events
    ///     .filter(|e| e.event == "my-event")
    ///     .for_each(|e| println!("{:?}", e))
    ///     .await;
    /// ```
    ///
    /// # Performance
    ///
    /// The broadcast channel uses `Arc` internally, so cloning events is zero-copy.
    /// Multiple subscribers can listen to the same event stream without performance penalty.
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.event_broadcast.subscribe()
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
        assert_eq!(
            client.get_connection_state().await,
            ConnectionState::Disconnected
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_generate_shared_secret() {
        let config =
            PusherConfig::from_env().expect("Failed to load Pusher configuration from environment");
        let client = PusherClient::new(config).unwrap();
        let secret = client.generate_shared_secret("test-channel");
        assert!(!secret.is_empty());
    }

    #[tokio::test]
    async fn test_trigger_batch() {
        let config =
            PusherConfig::from_env().expect("Failed to load Pusher configuration from environment");
        let client = PusherClient::new(config).unwrap();

        let batch_events = vec![
            BatchEvent {
                channel: "test-channel-1".to_string(),
                event: "test-event-1".to_string(),
                data: "{\"message\": \"Hello from event 1\"}".to_string(),
            },
            BatchEvent {
                channel: "test-channel-2".to_string(),
                event: "test-event-2".to_string(),
                data: "{\"message\": \"Hello from event 2\"}".to_string(),
            },
        ];

        let result = client.trigger_batch(batch_events).await;
        assert!(result.is_ok());
    }
}
