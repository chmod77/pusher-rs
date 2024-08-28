use tokio_tungstenite::{
    connect_async, 
    tungstenite::protocol::Message,
    WebSocketStream,
    MaybeTlsStream
};
use tokio::net::TcpStream;
use futures_util::{SinkExt, StreamExt};
use tokio::time::{sleep, Duration, Instant};
use url::Url;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use log::{debug, error, info, warn};

use crate::error::{PusherError, PusherResult};
use crate::{Event, SystemEvent, ConnectionState};

const PING_INTERVAL: Duration = Duration::from_secs(30);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RECONNECTION_ATTEMPTS: u32 = 6;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);

pub struct WebSocketClient {
    url: Url,
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    state: Arc<RwLock<ConnectionState>>,
    event_tx: mpsc::Sender<Event>,
    command_rx: mpsc::Receiver<WebSocketCommand>,
    command_tx: mpsc::Sender<WebSocketCommand>,
}

pub enum WebSocketCommand {
    Send(String),
    Close,
}

impl WebSocketClient {
    pub fn new(url: Url, state: Arc<RwLock<ConnectionState>>, event_tx: mpsc::Sender<Event>) -> Self {
        let (command_tx, command_rx) = mpsc::channel(100);
        Self {
            url,
            socket: None,
            state,
            event_tx,
            command_rx,
            command_tx,
        }
    }

    pub fn get_command_tx(&self) -> mpsc::Sender<WebSocketCommand> {
        self.command_tx.clone()
    }

    pub async fn connect(&mut self) -> PusherResult<()> {
        debug!("Connecting to WebSocket: {}", self.url);
        let url_string = self.url.to_string();
        let (socket, _) = connect_async(url_string).await
            .map_err(|e| PusherError::WebSocketError(format!("Failed to connect: {}", e)))?;
        self.socket = Some(socket);
        self.set_state(ConnectionState::Connected).await;
        Ok(())
    }

    pub async fn run(&mut self) {
        let mut reconnection_attempts = 0;
        let mut backoff = INITIAL_BACKOFF;

        loop {
            match self.run_connection().await {
                Ok(_) => {
                    info!("WebSocket connection closed normally");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    self.handle_disconnect().await;

                    if reconnection_attempts >= MAX_RECONNECTION_ATTEMPTS {
                        error!("Max reconnection attempts reached. Giving up.");
                        break;
                    }

                    reconnection_attempts += 1;
                    info!("Attempting to reconnect in {:?} (attempt {})", backoff, reconnection_attempts);
                    tokio::time::sleep(backoff).await;
                    backoff *= 2; // Exponential backoff

                    if let Err(e) = self.connect().await {
                        error!("Failed to reconnect: {}", e);
                        continue;
                    }
                }
            }
        }
    }

    async fn run_connection(&mut self) -> PusherResult<()> {
        let mut ping_interval = Instant::now() + PING_INTERVAL;
        let mut pong_timeout: Option<Instant> = None;

        while let Some(socket) = &mut self.socket {
            tokio::select! {
                cmd = self.command_rx.recv() => {
                    match cmd {
                        Some(WebSocketCommand::Send(msg)) => {
                            socket.send(Message::Text(msg)).await
                                .map_err(|e| PusherError::WebSocketError(format!("Failed to send message: {}", e)))?;
                        }
                        Some(WebSocketCommand::Close) => {
                            debug!("Closing WebSocket connection");
                            socket.close(None).await
                                .map_err(|e| PusherError::WebSocketError(format!("Failed to close connection: {}", e)))?;
                            return Ok(());
                        }
                        None => {
                            return Err(PusherError::WebSocketError("Command channel closed".to_string()));
                        }
                    }
                }
                message = socket.next() => {
                    match message {
                        Some(Ok(msg)) => self.handle_message(msg).await?,
                        Some(Err(e)) => return Err(PusherError::WebSocketError(format!("WebSocket error: {}", e))),
                        None => {
                            info!("WebSocket connection closed");
                            return Ok(());
                        }
                    }
                }
                _ = sleep(ping_interval.duration_since(Instant::now())) => {
                    debug!("Sending ping");
                    socket.send(Message::Ping(vec![])).await
                        .map_err(|e| PusherError::WebSocketError(format!("Failed to send ping: {}", e)))?;
                    ping_interval = Instant::now() + PING_INTERVAL;
                    pong_timeout = Some(Instant::now() + PONG_TIMEOUT);
                }
                _ = async { 
                    if let Some(timeout) = pong_timeout {
                        sleep(timeout.duration_since(Instant::now())).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    if pong_timeout.is_some() {
                        warn!("Pong timeout reached");
                        return Err(PusherError::WebSocketError("Pong timeout reached".to_string()));
                    }
                }
            }

            if let Some(timeout) = pong_timeout {
                if Instant::now() >= timeout {
                    pong_timeout = None;
                }
            }
        }

        Ok(())
    }
    async fn handle_message(&mut self, msg: Message) -> PusherResult<()> {
        match msg {
            Message::Text(text) => self.handle_text_message(text).await,
            Message::Ping(_) => self.handle_ping().await,
            Message::Pong(_) => Ok(self.handle_pong()),
            Message::Close(frame) => self.handle_close(frame).await,
            _ => {
                debug!("Received unhandled message type");
                Ok(())
            }
        }
    }

    async fn handle_text_message(&self, text: String) -> PusherResult<()> {
        debug!("Received text message: {}", text);
        match serde_json::from_str::<SystemEvent>(&text) {
            Ok(system_event) => self.handle_system_event(system_event).await,
            Err(_) => {
                if let Ok(event) = serde_json::from_str::<Event>(&text) {
                    self.event_tx.send(event).await
                        .map_err(|e| PusherError::WebSocketError(format!("Failed to send event to handler: {}", e)))?;
                } else {
                    error!("Failed to parse message as Event or SystemEvent: {}", text);
                }
                Ok(())
            }
        }
    }

    async fn handle_system_event(&self, event: SystemEvent) -> PusherResult<()> {
        match event.event.as_str() {
            "pusher:connection_established" => {
                info!("Connection established");
                self.set_state(ConnectionState::Connected).await;
            }
            "pusher:error" => {
                error!("Received error event: {:?}", event.data);
                // Handle specific error cases here
            }
            _ => debug!("Received unhandled system event: {}", event.event),
        }
        Ok(())
    }

    async fn handle_ping(&mut self) -> PusherResult<()> {
        debug!("Received ping, sending pong");
        if let Some(socket) = &mut self.socket {
            socket.send(Message::Pong(vec![])).await
                .map_err(|e| PusherError::WebSocketError(format!("Failed to send pong: {}", e)))?;
        }
        Ok(())
    }

    fn handle_pong(&mut self) {
        debug!("Received pong");
        // Reset pong timeout here if implemented
    }

    async fn handle_close(&mut self, frame: Option<tokio_tungstenite::tungstenite::protocol::CloseFrame<'_>>) -> PusherResult<()> {
        if let Some(frame) = frame {
            info!("WebSocket closed with code {} and reason: {}", frame.code, frame.reason);
        } else {
            info!("WebSocket closed without close frame");
        }
        self.handle_disconnect().await;
        Ok(())
    }

    async fn handle_disconnect(&mut self) {
        self.set_state(ConnectionState::Disconnected).await;
        self.socket = None;
    }

    async fn set_state(&self, new_state: ConnectionState) {
        let mut state = self.state.write().await;
        *state = new_state.clone();
        debug!("Connection state changed to: {:?}", new_state);
    }

    pub async fn send(&self, message: String) -> PusherResult<()> {
        self.command_tx.send(WebSocketCommand::Send(message)).await
            .map_err(|e| PusherError::WebSocketError(format!("Failed to send command: {}", e)))
    }

    pub async fn close(&self) -> PusherResult<()> {
        self.command_tx.send(WebSocketCommand::Close).await
            .map_err(|e| PusherError::WebSocketError(format!("Failed to send close command: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_websocket_client_creation() {
        let url = Url::parse("wss://ws.pusherapp.com/app/1234?protocol=7").unwrap();
        let state = Arc::new(RwLock::new(ConnectionState::Disconnected));
        let (event_tx, _) = mpsc::channel(100);

        let client = WebSocketClient::new(url, state, event_tx);
        assert!(client.socket.is_none());
    }

}