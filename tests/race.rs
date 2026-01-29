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

    // Subscribe to the channel
    match timeout(Duration::from_secs(5), client.subscribe("test-channel")).await {
        Ok(result) => {
            result.expect("Failed to subscribe to channel");
        }
        Err(_) => panic!("Subscription timed out"),
    }

    // Wait a bit for the subscription to be processed
    tokio::time::sleep(Duration::from_secs(1)).await;

    let channels = client.get_subscribed_channels().await;
    log::info!("Subscribed channels: {:?}", channels);
    assert!(channels.contains(&"test-channel".to_string()), "Channel not found in subscribed channels");
    let occupancy = client.get_channel_occupancy("my-channel").await;
    println!("Channel occupancy: {:?}", occupancy);
    // Unsubscribe from the channel
    match timeout(Duration::from_secs(5), client.unsubscribe("test-channel")).await {
        Ok(result) => {
            result.expect("Failed to unsubscribe from channel");
        }
        Err(_) => panic!("Unsubscription timed out"),
    }

    // Wait a bit for the unsubscription to be processed
    tokio::time::sleep(Duration::from_secs(1)).await;

    let channels = client.get_subscribed_channels().await;
    assert!(!channels.contains(&"test-channel".to_string()), "Channel still present after unsubscription");

    // Disconnect the client
    client.disconnect().await.expect("Failed to disconnect");
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
#[ignore]
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
    client
        .subscribe(test_channel)
        .await
        .expect("Failed to subscribe to channel");

    // Set up event binding to capture the triggered event
    let event_received = Arc::new(RwLock::new(false));
    let event_received_clone = event_received.clone();
    let received_data = Arc::new(RwLock::new(None));
    let received_data_clone = received_data.clone();

    client
        .bind(test_event, move |event: Event| {
            let event_received = event_received_clone.clone();
            let received_data = received_data_clone.clone();
            tokio::spawn(async move {
                let mut flag = event_received.write().await;
                *flag = true;
                let mut data = received_data.write().await;
                *data = Some(event.data);
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

    // let's ssert that the event was received and processed
    assert!(*event_received.read().await, "Event was not received");

    // let's assert that the received data matches the sent data
    let received = received_data.read().await;
    let expected_data: serde_json::Value = serde_json::from_str(test_data).unwrap();
    assert_eq!(
        received.as_ref().unwrap(),
        &expected_data,
        "Received data does not match sent data"
    );

    client
        .unsubscribe(test_channel)
        .await
        .expect("Failed to unsubscribe from channel");
    client.disconnect().await.expect("Failed to disconnect");
}

/// Test that reproduces the exact scenario from the user's initial code
/// This test demonstrates the race condition issue where subscribing first
/// and then binding the event handler can miss events
#[tokio::test]
async fn test_user_initial_code_race_condition() {
    println!("Testing the exact scenario from user's initial code...");
    
    let mut client = setup_client().await;
    
    // Track whether we receive any events
    let event_received = Arc::new(RwLock::new(false));
    let event_received_clone = event_received.clone();
    
    // This reproduces the user's exact code pattern:
    // 1. Connect
    // 2. Subscribe 
    // 3. Bind (AFTER subscribing - this is the race condition)
    
    println!("Connecting to Pusher...");
    client.connect().await.expect("Failed to connect");
    
    // Wait for connection to be established
    let mut connection_attempts = 0;
    while client.get_connection_state().await != ConnectionState::Connected {
        tokio::time::sleep(Duration::from_millis(100)).await;
        connection_attempts += 1;
        if connection_attempts > 50 { // 5 second timeout
            panic!("Connection timeout");
        }
    }
    println!("Connected!");
    
    println!("Subscribing to race-test-channel...");
    client.subscribe("race-test-channel").await.expect("Failed to subscribe");
    
    // Small delay to simulate the race condition window
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    println!("Binding event handler (AFTER subscribing - race condition!)...");
    client.bind("race-test-event", move |event| {
        println!("Received event in race condition test: {:#?}", event);
        let event_received = event_received_clone.clone();
        tokio::spawn(async move {
            let mut flag = event_received.write().await;
            *flag = true;
        });
    }).await.expect("Failed to bind event");
    
    println!("Binded");
    
    // Now trigger an event to see if our handler catches it
    println!("Triggering test event...");
    let test_data = r#"{"message": "Test from race condition test", "timestamp": "2025-01-01T00:00:00Z"}"#;
    
    match client.trigger("race-test-channel", "race-test-event", test_data).await {
        Ok(_) => println!("Event triggered successfully"),
        Err(e) => {
            println!("Failed to trigger event: {:?}", e);
            // Don't panic here, as the test should continue to show the race condition
        }
    }
    
    // Wait for potential event reception
    println!("Waiting for event reception (5 seconds)...");
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    let received = *event_received.read().await;
    
    if received {
        println!("SUCCESS: Event was received despite the race condition!");
        println!("   This suggests the library handles the race condition well,");
        println!("   or we got lucky with timing.");
    } else {
        println!("RACE CONDITION CONFIRMED: Event was NOT received!");
        println!("   This demonstrates the issue with binding after subscribing.");
        println!("   The event may have arrived before the handler was registered.");
    }
    
    // Clean up
    client.disconnect().await.expect("Failed to disconnect");
    
    // For this test, we'll assert that it demonstrates the issue
    // In a real scenario, this assertion might fail due to timing
    // Comment out this assertion if you want to observe the behavior
    // without failing the test
    assert!(
        received,
        "Race condition test: Event should have been received. \
         If this fails, it confirms the race condition issue in the user's code."
    );
}

/// Test that shows the CORRECT way to set up event handling
/// This demonstrates binding BEFORE subscribing to avoid race conditions
#[tokio::test]
async fn test_correct_event_handling_pattern() {
    println!("Testing the CORRECT event handling pattern...");
    
    let mut client = setup_client().await;
    
    let event_received = Arc::new(RwLock::new(false));
    let event_received_clone = event_received.clone();
    let received_data = Arc::new(RwLock::new(None));
    let received_data_clone = received_data.clone();
    
    // CORRECT PATTERN: Bind BEFORE connecting/subscribing
    println!("Binding event handler FIRST (correct pattern)...");
    client.bind("correct-test-event", move |event| {
        println!("Received event in correct pattern test: {:#?}", event);
        let event_received = event_received_clone.clone();
        let received_data = received_data_clone.clone();
        tokio::spawn(async move {
            let mut flag = event_received.write().await;
            *flag = true;
            let mut data = received_data.write().await;
            *data = Some(event.data);
        });
    }).await.expect("Failed to bind event");
    
    println!("Connecting to Pusher...");
    client.connect().await.expect("Failed to connect");
    
    // Wait for connection
    while client.get_connection_state().await != ConnectionState::Connected {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("Connected!");
    
    println!("Subscribing to correct-test-channel...");
    client.subscribe("correct-test-channel").await.expect("Failed to subscribe");
    
    // Wait a bit for subscription to be processed
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Trigger an event
    println!("Triggering test event...");
    let test_data = r#"{"message": "Test from correct pattern test", "success": true}"#;
    
    client.trigger("correct-test-channel", "correct-test-event", test_data).await
        .expect("Failed to trigger event");
    
    println!("Event triggered successfully");
    
    // Wait for event reception
    println!("Waiting for event reception (3 seconds)...");
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    let received = *event_received.read().await;
    let data = received_data.read().await;
    
    if received {
        println!("SUCCESS: Event was received with correct pattern!");
        if let Some(event_data) = data.as_ref() {
            println!("Received data: {}", event_data);
        }
    } else {
        println!("UNEXPECTED: Event was not received even with correct pattern!");
    }
    
    // Clean up
    client.disconnect().await.expect("Failed to disconnect");
    
    // This should succeed with the correct pattern
    assert!(received, "Correct pattern test: Event should have been received");
    
    // Verify the data content
    let expected_data: serde_json::Value = serde_json::from_str(test_data).unwrap();
    assert_eq!(
        data.as_ref().unwrap(),
        &expected_data,
        "Received data should match sent data"
    );
    
    println!("Correct pattern test completed successfully!");
}

/// Comprehensive test that demonstrates both patterns side by side
#[tokio::test]
async fn test_race_condition_comparison() {
    println!("Running comprehensive race condition comparison test...");
    
    // Test 1: Race condition pattern (subscribe then bind)
    println!("\n=== Test 1: Race Condition Pattern ===");
    let result1 = test_pattern_subscribe_then_bind().await;
    
    // Test 2: Correct pattern (bind then subscribe) 
    println!("\n=== Test 2: Correct Pattern ===");
    let result2 = test_pattern_bind_then_subscribe().await;
    
    println!("\n=== Results Summary ===");
    println!("Race condition pattern (subscribe→bind): {}", if result1 { "Received" } else { "Missed" });
    println!("Correct pattern (bind→subscribe): {}", if result2 { "Received" } else { "Missed" });
    
    // The correct pattern should always work
    assert!(result2, "Correct pattern should always receive events");
    
    // The race condition pattern might or might not work depending on timing
    if !result1 {
        println!("WARNING: Race condition confirmed: subscribe→bind pattern missed the event");
    } else {
        println!("INFO: Race condition pattern worked this time (timing dependent)");
    }
}

async fn test_pattern_subscribe_then_bind() -> bool {
    let mut client = setup_client().await;
    let event_received = Arc::new(RwLock::new(false));
    let event_received_clone = event_received.clone();
    
    client.connect().await.expect("Connect failed");
    while client.get_connection_state().await != ConnectionState::Connected {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    
    // RACE CONDITION: Subscribe first
    client.subscribe("race-test-channel").await.expect("Subscribe failed");
    tokio::time::sleep(Duration::from_millis(50)).await; // Simulate delay
    
    // Then bind
    client.bind("race-test-event", move |_event| {
        let event_received = event_received_clone.clone();
        tokio::spawn(async move {
            *event_received.write().await = true;
        });
    }).await.expect("Bind failed");
    
    // Trigger event
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = client.trigger("race-test-channel", "race-test-event", r#"{"test": "race"}"#).await;
    
    // Wait for result
    tokio::time::sleep(Duration::from_secs(2)).await;
    let result = *event_received.read().await;
    
    client.disconnect().await.ok();
    result
}

async fn test_pattern_bind_then_subscribe() -> bool {
    let mut client = setup_client().await;
    let event_received = Arc::new(RwLock::new(false));
    let event_received_clone = event_received.clone();
    
    // CORRECT: Bind first
    client.bind("correct-test-event", move |_event| {
        let event_received = event_received_clone.clone();
        tokio::spawn(async move {
            *event_received.write().await = true;
        });
    }).await.expect("Bind failed");
    
    client.connect().await.expect("Connect failed");
    while client.get_connection_state().await != ConnectionState::Connected {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    
    // Then subscribe
    client.subscribe("correct-test-channel").await.expect("Subscribe failed");
    
    // Trigger event
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = client.trigger("correct-test-channel", "correct-test-event", r#"{"test": "correct"}"#).await;
    
    // Wait for result
    tokio::time::sleep(Duration::from_secs(2)).await;
    let result = *event_received.read().await;
    
    client.disconnect().await.ok();
    result
}