use env_logger::Env;
use once_cell::sync::OnceCell;
use pusher_rs::{ConnectionState, Event, PusherClient, PusherConfig};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::timeout;
use tokio::time::{sleep, Duration};

static INIT: OnceCell<()> = OnceCell::new();

fn init_logger() {
    INIT.get_or_init(|| {
        let env = Env::default()
            .filter_or("MY_LOG_LEVEL", "debug")
            .write_style_or("MY_LOG_STYLE", "always");
        env_logger::init_from_env(env);
    });
}

async fn setup_client() -> PusherClient {
    init_logger();
    let config =
        PusherConfig::from_env().expect("Failed to load Pusher configuration from environment");
    PusherClient::new(config).unwrap()
}

#[tokio::test]
async fn test_pusher_client_connection() {
    let mut client = setup_client().await;

    client.connect().await.unwrap();
    assert_eq!(
        client.get_connection_state().await,
        ConnectionState::Connected
    );

    client.disconnect().await.unwrap();
    assert_eq!(
        client.get_connection_state().await,
        ConnectionState::Disconnected
    );
}

#[tokio::test]
async fn test_channel_subscription() {
    let mut client = setup_client().await;

    client.connect().await.unwrap();
    client.subscribe("test-channel").await.unwrap();

    let channels = client.get_subscribed_channels().await;
    assert!(channels.contains(&"test-channel".to_string()));

    client.unsubscribe("test-channel").await.unwrap();

    let channels = client.get_subscribed_channels().await;
    assert!(!channels.contains(&"test-channel".to_string()));
}

#[tokio::test]
async fn test_event_binding() {
    let client = setup_client().await;

    let event_received = Arc::new(RwLock::new(false));
    let event_received_clone = event_received.clone();

    client
        .bind("test-event", move |_event: Event| {
            let event_received = event_received_clone.clone();
            tokio::spawn(async move {
                let mut flag = event_received.write().await;
                *flag = true;
            });
        })
        .await
        .unwrap();

    let event = Event::new("test-event".to_string(), None, serde_json::json!({}));
    client.send_test_event(event).await.unwrap();

    sleep(Duration::from_millis(100)).await;

    assert!(*event_received.read().await);
}

#[tokio::test]
async fn test_encrypted_channel() {
    let mut client = setup_client().await;

    client.connect().await.unwrap();
    client
        .subscribe_encrypted("private-encrypted-channel")
        .await
        .unwrap();

    let channels = client.get_subscribed_channels().await;
    assert!(channels.contains(&"private-encrypted-channel".to_string()));

    // TODO - Test sending and receiving encrypted messages
}

#[tokio::test]
async fn test_send_payload() {
    let mut client = setup_client().await;

    // Connect with a timeout
    match timeout(Duration::from_secs(10), client.connect()).await {
        Ok(result) => {
            result.expect("Failed to connect to Pusher");
        }
        Err(_) => panic!("Connection timed out"),
    }

    // Ensure we're connected
    assert_eq!(
        client.get_connection_state().await,
        ConnectionState::Connected
    );

    let test_channel = "test-channel-payload";
    let test_event = "test-event-payload";
    let test_data = r#"{"message": "Hello, Pusher!"}"#;

    // Subscribe to the channel
    // client
    //     .subscribe(test_channel)
    //     .await
    //     .expect("Failed to subscribe to channel");

    // Set up event binding to capture the triggered event
    let event_received = Arc::new(RwLock::new(false));
    let event_received_clone = event_received.clone();
    let received_data = Arc::new(RwLock::new(String::new()));
    let received_data_clone = received_data.clone();

    client
        .bind(test_event, move |event: Event| {
            let event_received = event_received_clone.clone();
            let received_data = received_data_clone.clone();
            tokio::spawn(async move {
                let mut flag = event_received.write().await;
                *flag = true;
                let mut data = received_data.write().await;
                *data = serde_json::to_string(&event.data).unwrap();
            });
        })
        .await
        .expect("Failed to bind event");

    // Trigger the event
    match client.trigger(test_channel, test_event, test_data).await {
        Ok(_) => println!("Event triggered successfully"),
        Err(e) => panic!("Failed to trigger event: {:?}", e),
    }

    // Wait for the event to be processed
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Assert that the event was received and processed
    // assert!(*event_received.read().await, "Event was not received");

    // Assert that the received data matches the sent data
    // let received = received_data.read().await;
    // assert_eq!(
    //     *received, test_data,
    //     "Received data does not match sent data"
    // );

    // client
    //     .unsubscribe(test_channel)
    //     .await
    //     .expect("Failed to unsubscribe from channel");
    client.disconnect().await.expect("Failed to disconnect");
}
