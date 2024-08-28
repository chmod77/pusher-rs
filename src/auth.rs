use crate::error::{PusherError, PusherResult};
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use log::debug;
use md5::compute;
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

pub struct PusherAuth {
    key: String,
    secret: String,
}

impl PusherAuth {
    pub fn new(key: &str, secret: &str) -> Self {
        Self {
            key: key.to_string(),
            secret: secret.to_string(),
        }
    }

    pub fn authenticate_socket(&self, socket_id: &str, channel_name: &str) -> PusherResult<String> {
        let auth_signature = self.sign_socket(socket_id, channel_name)?;
        Ok(format!("{}:{}", self.key, auth_signature))
    }

    pub fn authenticate_presence_channel(
        &self,
        socket_id: &str,
        channel_name: &str,
        user_id: &str,
        user_info: Option<&Value>,
    ) -> PusherResult<String> {
        let mut channel_data = json!({
            "user_id": user_id,
        });

        if let Some(info) = user_info {
            channel_data["user_info"] = info.clone();
        }

        let channel_data_str = serde_json::to_string(&channel_data)?;
        let auth_signature =
            self.sign_socket_with_channel_data(socket_id, channel_name, &channel_data_str)?;

        Ok(format!(
            "{}:{}:{}",
            self.key, auth_signature, channel_data_str
        ))
    }

    pub fn authenticate_private_encrypted_channel(
        &self,
        socket_id: &str,
        channel_name: &str,
    ) -> PusherResult<String> {
        let shared_secret = self.generate_shared_secret(channel_name);
        let auth_signature = self.sign_socket(socket_id, channel_name)?;
        Ok(format!(
            "{}:{}:{}",
            self.key,
            auth_signature,
            general_purpose::STANDARD.encode(shared_secret)
        ))
    }

    pub fn authenticate_request(
        &self,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> PusherResult<BTreeMap<String, String>> {
        let mut params = BTreeMap::new();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| PusherError::AuthError(e.to_string()))?
            .as_secs()
            .to_string();

        params.insert("auth_key".to_string(), self.key.clone());
        params.insert("auth_timestamp".to_string(), timestamp);
        params.insert("auth_version".to_string(), "1.0".to_string());

        let body_md5 = format!("{:x}", md5::compute(serde_json::to_string(body)?));
        params.insert("body_md5".to_string(), body_md5);
        let to_sign = self.create_signing_string(method, path, &params)?;

        let signature = self.sign(&to_sign)?;

        params.insert("auth_signature".to_string(), signature);

        Ok(params)
    }

    fn sign_socket(&self, socket_id: &str, channel_name: &str) -> PusherResult<String> {
        let to_sign = format!("{}:{}", socket_id, channel_name);
        self.sign(&to_sign)
    }

    fn sign_socket_with_channel_data(
        &self,
        socket_id: &str,
        channel_name: &str,
        channel_data: &str,
    ) -> PusherResult<String> {
        let to_sign = format!("{}:{}:{}", socket_id, channel_name, channel_data);
        self.sign(&to_sign)
    }

    fn sign(&self, to_sign: &str) -> PusherResult<String> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes())
            .map_err(|e| PusherError::AuthError(e.to_string()))?;
        mac.update(to_sign.as_bytes());
        let result = mac.finalize();
        Ok(hex::encode(result.into_bytes()))
    }

    fn create_signing_string(
        &self,
        method: &str,
        path: &str,
        params: &BTreeMap<String, String>,
    ) -> PusherResult<String> {
        let mut query_string: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        query_string.sort(); // Sort alphabetically
        let query_string = query_string.join("&");
        debug!("path: {}", path);
        Ok(format!("{}\n{}\n{}", method, path, query_string))
    }

    fn generate_shared_secret(&self, channel_name: &str) -> Vec<u8> {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(self.secret.as_bytes());
        hasher.update(channel_name.as_bytes());
        hasher.finalize().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authenticate_socket() {
        let auth = PusherAuth::new("key", "secret");
        let result = auth.authenticate_socket("socket_id", "channel_name");
        assert!(result.is_ok());
    }

    #[test]
    fn test_authenticate_presence_channel() {
        let auth = PusherAuth::new("key", "secret");
        let result = auth.authenticate_presence_channel(
            "socket_id",
            "presence-channel",
            "user_id",
            Some(&serde_json::json!({"name": "Test User"}))
        );
        assert!(result.is_ok());
    }

}