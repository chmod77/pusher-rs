use tokio_tungstenite::{
    connect_async, 
    tungstenite::protocol::Message,
    WebSocketStream,
    MaybeTlsStream
};
use tokio::net::TcpStream;
use futures_util::{SinkExt, StreamExt};
use tokio::time::{sleep, interval, Duration};
use url::Url;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use log::{debug, error, info, warn};
use std::pin::Pin;

use crate::error::{PusherError, PusherResult};
use crate::{Event, SystemEvent, ConnectionState};

const PING_INTERVAL: Duration = Duration::from_secs(30);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);

pub struct WebSocketClient {
    url: Url,
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    state: Arc<RwLock<ConnectionState>>,
    event_tx: mpsc::Sender<Event>,
    command_rx: mpsc::Receiver<WebSocketCommand>,
}

pub enum WebSocketCommand {
    Send(String),
    Close,
}

impl WebSocketClient {
    pub fn new(
        url: Url,
        state: Arc<RwLock<ConnectionState>>,
        event_tx: mpsc::Sender<Event>,
        command_rx: mpsc::Receiver<WebSocketCommand>,
    ) -> Self {
        Self {
            url,
            socket: None,
            state,
            event_tx,
            command_rx,
        }
    }

    pub async fn connect(&mut self) -> PusherResult<()> {
        debug!("Connecting to WebSocket: {}", self.url);
        let (socket, _) = connect_async(self.url.to_string()).await
            .map_err(|e| PusherError::WebSocketError(format!("Failed to connect: {}", e)))?;
        self.socket = Some(socket);
        self.set_state(ConnectionState::Connected).await;
        Ok(())
    }

    pub async fn run(&mut self) {
        let mut ping_interval = interval(PING_INTERVAL);
        let mut pong_timeout = Box::pin(sleep(Duration::from_secs(0)));
        let mut waiting_for_pong = false;

        while let Some(socket) = &mut self.socket {
            tokio::select! {
                _ = ping_interval.tick() => {
                    if let Err(e) = socket.send(Message::Ping(vec![])).await {
                        error!("Failed to send ping: {}", e);
                        break;
                    }
                    waiting_for_pong = true;
                    pong_timeout = Box::pin(sleep(PONG_TIMEOUT));
                }
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        WebSocketCommand::Send(msg) => {
                            if let Err(e) = socket.send(Message::Text(msg)).await {
                                error!("Failed to send message: {}", e);
                            }
                        }
                        WebSocketCommand::Close => {
                            if let Err(e) = socket.close(None).await {
                                error!("Failed to close connection: {}", e);
                            }
                            break;
                        }
                    }
                }
                msg = socket.next() => {
                    match msg {
                        Some(Ok(msg)) => {
                            if let Message::Pong(_) = msg {
                                waiting_for_pong = false;
                            }
                            self.handle_message(msg).await;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket connection closed");
                            break;
                        }
                    }
                }
                _ = &mut pong_timeout, if waiting_for_pong => {
                    error!("Pong timeout reached");
                    break;
                }
            }
        }

        self.handle_disconnect().await;
    }

    async fn handle_message(&mut self, msg: Message) {
        match msg {
            Message::Text(text) => self.handle_text_message(text).await,
            Message::Ping(_) => {
                if let Some(socket) = &mut self.socket {
                    if let Err(e) = socket.send(Message::Pong(vec![])).await {
                        error!("Failed to send pong: {}", e);
                    }
                }
            }
            Message::Pong(_) => {
                debug!("Received pong");
            }
            Message::Close(frame) => {
                info!("Received close frame: {:?}", frame);
                self.handle_disconnect().await;
            }
            _ => {
                debug!("Received unhandled message type");
            }
        }
    }

    async fn handle_text_message(&self, text: String) {
        debug!("Received text message: {}", text);
        if let Ok(event) = serde_json::from_str::<Event>(&text) {
            if let Err(e) = self.event_tx.send(event).await {
                error!("Failed to send event to handler: {}", e);
            }
        } else {
            error!("Failed to parse message as Event: {}", text);
        }
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
}