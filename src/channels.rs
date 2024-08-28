use crate::error::PusherResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelType {
    Public,
    Private,
    Presence,
    PrivateEncrypted,
}

#[derive(Debug, Clone)]
pub struct Channel {
    name: String,
    channel_type: ChannelType,
    subscribed: bool,
    members: Option<HashMap<String, serde_json::Value>>,
}

impl Channel {
    pub fn new(name: &str) -> Self {
        let channel_type = if name.starts_with("private-encrypted-") {
            ChannelType::PrivateEncrypted
        } else if name.starts_with("private-") {
            ChannelType::Private
        } else if name.starts_with("presence-") {
            ChannelType::Presence
        } else {
            ChannelType::Public
        };

        Self {
            name: name.to_string(),
            channel_type: channel_type.clone(),
            subscribed: false,
            members: if channel_type == ChannelType::Presence {
                Some(HashMap::new())
            } else {
                None
            },
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channel_type(&self) -> &ChannelType {
        &self.channel_type
    }

    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }

    pub fn set_subscribed(&mut self, subscribed: bool) {
        self.subscribed = subscribed;
    }

    pub fn members(&self) -> Option<&HashMap<String, serde_json::Value>> {
        self.members.as_ref()
    }

    pub fn add_member(&mut self, id: String, info: serde_json::Value) -> PusherResult<()> {
        if let Some(members) = &mut self.members {
            members.insert(id, info);
            Ok(())
        } else {
            Err(crate::error::channel_error("Not a presence channel"))
        }
    }

    pub fn remove_member(&mut self, id: &str) -> PusherResult<()> {
        if let Some(members) = &mut self.members {
            members.remove(id);
            Ok(())
        } else {
            Err(crate::error::channel_error("Not a presence channel"))
        }
    }

    pub fn clear_members(&mut self) {
        if let Some(members) = &mut self.members {
            members.clear();
        }
    }

    pub fn member_count(&self) -> usize {
        self.members.as_ref().map_or(0, |m| m.len())
    }
}

pub struct ChannelList {
    channels: HashMap<String, Channel>,
}

impl ChannelList {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    pub fn add(&mut self, channel: Channel) {
        self.channels.insert(channel.name().to_string(), channel);
    }

    pub fn remove(&mut self, channel_name: &str) -> Option<Channel> {
        self.channels.remove(channel_name)
    }

    pub fn get(&self, channel_name: &str) -> Option<&Channel> {
        self.channels.get(channel_name)
    }

    pub fn get_mut(&mut self, channel_name: &str) -> Option<&mut Channel> {
        self.channels.get_mut(channel_name)
    }

    pub fn contains(&self, channel_name: &str) -> bool {
        self.channels.contains_key(channel_name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Channel> {
        self.channels.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Channel> {
        self.channels.values_mut()
    }
}

impl Default for ChannelList {
    fn default() -> Self {
        Self::new()
    }
}
