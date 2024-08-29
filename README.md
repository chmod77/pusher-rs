# Welcome to `pusher_rs` - The long-missing Rust Pusher crate.

This library is still under heavy development. Currently in use by one of the projects I am working on. Open-sourcing it since it has matured to a good level. 

A Rust client library for interacting with the Pusher Channels API. This library provides a simple and efficient way to integrate Pusher's real-time functionality into your Rust applications.

## Features

- [x] Connect to Pusher Channels
- [x] Subscribe to public, private, presence, and private encrypted channels
- [x] Publish events to channels
- [x] Handle incoming events
- [x] Automatic reconnection with exponential backoff
- [x] Environment-based configuration
- [x] Flexible channel management

### Todo
- [ ] Support for Batch Triggers
- [ ] Improve error handling and logging
- [ ] Add comprehensive test suite
- [ ] Implement WebHooks support
- [ ] Optimize performance for high-load scenarios
- [ ] Create more detailed documentation and examples


## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
pusher-rs = { git = "https://github.com/chmod77/pusher-rs.git" }
```

If you want to use a specific branch or commit, you can specify it like this:
```toml
[dependencies]
pusher-rs = { git = "https://github.com/username/pusher-rs.git", branch = "main" }
# or
pusher-rs = { git = "https://github.com/username/pusher-rs.git", rev = "commit_hash" }
```

## Configuration

The library uses environment variables for configuration. Create a `.env` file in your project root with the following variables:

```
PUSHER_APP_ID=your_app_id
PUSHER_KEY=your_app_key
PUSHER_SECRET=your_app_secret
PUSHER_CLUSTER=your_cluster
PUSHER_USE_TLS=true - this will enforce TLS
```

## Usage

### Initializing the client

```rust
use pusher_rs::{PusherClient, PusherConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = PusherConfig::from_env()?;
    let mut client = PusherClient::new(config)?;

    // PusherClient::new(config).unwrap()
    
    client.connect().await?;

    Ok(())
}
```

### Subscribing to a channel

```rust
client.subscribe("my-channel").await?;
```

### Publishing an event

```rust
client.trigger("my-channel", "my-event", "Hello, Pusher!").await?;
```

### Handling events

```rust
client.bind("my-event", |event| {
    println!("Received event: {:?}", event);
}).await?;
```

### Working with encrypted channels

```rust
// Subscribe to an encrypted channel
client.subscribe_encrypted("private-encrypted-channel").await?;

// Publish to an encrypted channel
client.trigger_encrypted("private-encrypted-channel", "my-event", "Secret message").await?;
```

## Channel Types

The library supports four types of channels:

- Public
- Private
- Presence
- Private Encrypted

Each channel type has specific features and authentication requirements.

## Error Handling

The library uses a custom `PusherError` type for error handling. You can match on different error variants to handle specific error cases:

```rust
use pusher_rs::PusherError;

match client.connect().await {
    Ok(_) => println!("Connected successfully"),
    Err(PusherError::ConnectionError(e)) => println!("Connection error: {}", e),
    Err(PusherError::AuthError(e)) => println!("Authentication error: {}", e),
    Err(e) => println!("Other error: {}", e),
}
```

## Advanced Usage

### Custom Configuration

While the library defaults to using environment variables, you can also create a custom configuration:

```rust
use pusher_rs::PusherConfig;
use std::time::Duration;

let config = PusherConfig {
    app_id: "your_app_id".to_string(),
    app_key: "your_app_key".to_string(),
    app_secret: "your_app_secret".to_string(),
    cluster: "your_cluster".to_string(),
    use_tls: true,
    host: Some("custom.pusher.com".to_string()),
    max_reconnection_attempts: 10,
    backoff_interval: Duration::from_secs(2),
    activity_timeout: Duration::from_secs(180),
    pong_timeout: Duration::from_secs(45),
};
```

### Channel Management

The library provides a `ChannelList` struct for managing subscribed channels:

```rust
let mut channel_list = ChannelList::new();
let channel = Channel::new("my-channel");
channel_list.add(channel);

if let Some(channel) = channel_list.get("my-channel") {
    println!("Channel type: {:?}", channel.channel_type());
}
```

### Tests

Integration tests live under `tests/integration_tests`

Just run `cargo test --test integration_tests -- --nocapture` to start.

More tests are being added. This section will be updated accordingly.


## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. Here's how you can contribute:

- [x] Fork the repository
- [x] Create your feature branch (`git checkout -b feat/amazing-feature`)
- [x] Commit your changes (`git commit -m 'feat: Add some amazing feature'`)
- [x] Push to the branch (`git push origin feat/amazing-feature`)
- [x] Open a Pull Request

## License

This project is licensed under the MIT License.
