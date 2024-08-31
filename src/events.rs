use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    pub channel: Option<String>,
    #[serde(with = "json_string")]
    pub data: Value,
}

impl Event {
    pub fn new(event: String, channel: Option<String>, data: Value) -> Self {
        Self {
            event,
            channel,
            data,
        }
    }

    pub fn is_system_event(&self) -> bool {
        self.event.starts_with("pusher:")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub event: String,
    pub channel: Option<String>,
    pub data: SystemEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemEventData {
    ConnectionEstablished {
        socket_id: String,
        activity_timeout: u64,
    },
    SubscriptionSucceeded {
        #[serde(default)]
        presence: Option<PresenceData>,
    },
    MemberAdded {
        user_id: String,
        user_info: Value,
    },
    MemberRemoved {
        user_id: String,
    },
    Error {
        code: Option<u32>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceData {
    pub count: u32,
    pub hash: std::collections::HashMap<String, Value>,
    pub ids: Vec<String>,
}

impl SystemEvent {
    pub fn new(event: String, channel: Option<String>, data: SystemEventData) -> Self {
        Self {
            event,
            channel,
            data,
        }
    }

    pub fn connection_established(socket_id: String, activity_timeout: u64) -> Self {
        Self::new(
            "pusher:connection_established".to_string(),
            None,
            SystemEventData::ConnectionEstablished {
                socket_id,
                activity_timeout,
            },
        )
    }

    pub fn subscription_succeeded(channel: String, presence: Option<PresenceData>) -> Self {
        Self::new(
            "pusher:subscription_succeeded".to_string(),
            Some(channel),
            SystemEventData::SubscriptionSucceeded { presence },
        )
    }

    pub fn member_added(channel: String, user_id: String, user_info: Value) -> Self {
        Self::new(
            "pusher:member_added".to_string(),
            Some(channel),
            SystemEventData::MemberAdded { user_id, user_info },
        )
    }

    pub fn member_removed(channel: String, user_id: String) -> Self {
        Self::new(
            "pusher:member_removed".to_string(),
            Some(channel),
            SystemEventData::MemberRemoved { user_id },
        )
    }

    pub fn error(code: Option<u32>, message: String) -> Self {
        Self::new(
            "pusher:error".to_string(),
            None,
            SystemEventData::Error { code, message },
        )
    }
}

mod json_string {
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    pub fn serialize<S>(value: &Value, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.to_string().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        serde_json::from_str(&s).map_err(D::Error::custom)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_event_serialization() {
        let event = Event::new(
            "test_event".to_string(),
            Some("test_channel".to_string()),
            json!({"message": "Hello, world!"}),
        );

        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: Event = serde_json::from_str(&serialized).unwrap();

        assert_eq!(event.event, deserialized.event);
        assert_eq!(event.channel, deserialized.channel);
        assert_eq!(event.data, deserialized.data);
    }

    #[test]
    fn test_system_event_connection_established() {
        let event = SystemEvent::connection_established("socket123".to_string(), 120);

        assert_eq!(event.event, "pusher:connection_established");
        assert_eq!(event.channel, None);

        if let SystemEventData::ConnectionEstablished { socket_id, activity_timeout } = event.data {
            assert_eq!(socket_id, "socket123");
            assert_eq!(activity_timeout, 120);
        } else {
            panic!("Unexpected event data");
        }
    }

    #[test]
    fn test_system_event_subscription_succeeded() {
        let presence_data = PresenceData {
            count: 2,
            hash: [
                ("user1".to_string(), json!({"name": "Alice"})),
                ("user2".to_string(), json!({"name": "Bob"})),
            ].into_iter().collect(),
            ids: vec!["user1".to_string(), "user2".to_string()],
        };

        let event = SystemEvent::subscription_succeeded("presence-channel".to_string(), Some(presence_data));

        assert_eq!(event.event, "pusher:subscription_succeeded");
        assert_eq!(event.channel, Some("presence-channel".to_string()));

        if let SystemEventData::SubscriptionSucceeded { presence } = event.data {
            assert!(presence.is_some());
            let presence = presence.unwrap();
            assert_eq!(presence.count, 2);
            assert_eq!(presence.ids, vec!["user1", "user2"]);
        } else {
            panic!("Unexpected event data");
        }
    }

    #[test]
    fn test_system_event_member_added() {
        let event = SystemEvent::member_added(
            "presence-channel".to_string(),
            "user3".to_string(),
            json!({"name": "Charlie"}),
        );

        assert_eq!(event.event, "pusher:member_added");
        assert_eq!(event.channel, Some("presence-channel".to_string()));

        if let SystemEventData::MemberAdded { user_id, user_info } = event.data {
            assert_eq!(user_id, "user3");
            assert_eq!(user_info, json!({"name": "Charlie"}));
        } else {
            panic!("Unexpected event data");
        }
    }

    #[test]
    fn test_system_event_member_removed() {
        let event = SystemEvent::member_removed("presence-channel".to_string(), "user2".to_string());

        assert_eq!(event.event, "pusher:member_removed");
        assert_eq!(event.channel, Some("presence-channel".to_string()));

        if let SystemEventData::MemberRemoved { user_id } = event.data {
            assert_eq!(user_id, "user2");
        } else {
            panic!("Unexpected event data");
        }
    }

    #[test]
    fn test_system_event_error() {
        let event = SystemEvent::error(Some(4004), "Error message".to_string());

        assert_eq!(event.event, "pusher:error");
        assert_eq!(event.channel, None);

        if let SystemEventData::Error { code, message } = event.data {
            assert_eq!(code, Some(4004));
            assert_eq!(message, "Error message");
        } else {
            panic!("Unexpected event data");
        }
    }
}


impl Event {
    pub fn is_presence_event(&self) -> bool {
        matches!(self.event.as_str(), "pusher:member_added" | "pusher:member_removed")
    }

    pub fn is_subscription_event(&self) -> bool {
        self.event == "pusher:subscription_succeeded" || self.event == "pusher:subscription_error"
    }

    pub fn as_system_event(&self) -> Option<SystemEvent> {
        if self.is_system_event() {
            serde_json::from_value(serde_json::to_value(self).unwrap()).ok()
        } else {
            None
        }
    }
}


impl SystemEvent {
    pub fn is_presence_event(&self) -> bool {
        matches!(self.event.as_str(), "pusher:member_added" | "pusher:member_removed")
    }

    pub fn is_subscription_event(&self) -> bool {
        self.event == "pusher:subscription_succeeded" || self.event == "pusher:subscription_error"
    }

    pub fn as_event(&self) -> Event {
        Event {
            event: self.event.clone(),
            channel: self.channel.clone(),
            data: serde_json::to_value(&self.data).unwrap(),
        }
    }
}