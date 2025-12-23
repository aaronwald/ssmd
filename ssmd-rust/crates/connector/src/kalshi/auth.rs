use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::pss::SigningKey;
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use rsa::RsaPrivateKey;
use sha2::Sha256;
use thiserror::Error;

/// Authentication credentials for Kalshi API
pub struct KalshiCredentials {
    pub api_key: String,
    private_key: RsaPrivateKey,
}

/// Errors that can occur during authentication
#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Failed to parse private key: {0}")]
    PrivateKeyParse(String),
    #[error("Failed to sign message: {0}")]
    SigningError(String),
}

impl KalshiCredentials {
    /// Create new credentials from an API key and PEM-encoded private key
    ///
    /// Supports both PKCS#8 (-----BEGIN PRIVATE KEY-----) and
    /// PKCS#1 (-----BEGIN RSA PRIVATE KEY-----) formats
    pub fn new(api_key: String, private_key_pem: &str) -> Result<Self, AuthError> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(private_key_pem)
            .or_else(|_| RsaPrivateKey::from_pkcs1_pem(private_key_pem))
            .map_err(|e| AuthError::PrivateKeyParse(e.to_string()))?;
        Ok(Self {
            api_key,
            private_key,
        })
    }

    /// Sign a WebSocket request for Kalshi authentication
    ///
    /// Returns a tuple of (timestamp, signature) where:
    /// - timestamp: current time in milliseconds as a string
    /// - signature: base64-encoded RSA-PSS signature
    pub fn sign_websocket_request(&self) -> Result<(String, String), AuthError> {
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let message = format!("{}GET/trade-api/ws/v2", timestamp);
        let signature = self.sign_message(&message)?;
        Ok((timestamp, signature))
    }

    /// Sign a message using RSA-PSS with SHA256
    fn sign_message(&self, message: &str) -> Result<String, AuthError> {
        let signing_key = SigningKey::<Sha256>::new(self.private_key.clone());
        let mut rng = rand::thread_rng();
        let signature = signing_key
            .try_sign_with_rng(&mut rng, message.as_bytes())
            .map_err(|e| AuthError::SigningError(e.to_string()))?;
        Ok(BASE64.encode(signature.to_bytes()))
    }
}

impl std::fmt::Debug for KalshiCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KalshiCredentials")
            .field("api_key", &self.api_key)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::pkcs8::EncodePrivateKey;

    fn generate_test_key() -> RsaPrivateKey {
        let mut rng = rand::thread_rng();
        RsaPrivateKey::new(&mut rng, 2048).unwrap()
    }

    #[test]
    fn test_new_with_pkcs8_pem() {
        let key = generate_test_key();
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();

        let credentials = KalshiCredentials::new("test_api_key".to_string(), pem.as_str());
        assert!(credentials.is_ok());

        let credentials = credentials.unwrap();
        assert_eq!(credentials.api_key, "test_api_key");
    }

    #[test]
    fn test_new_with_pkcs1_pem() {
        let key = generate_test_key();
        let pem = key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF).unwrap();

        let credentials = KalshiCredentials::new("test_api_key".to_string(), pem.as_str());
        assert!(credentials.is_ok());

        let credentials = credentials.unwrap();
        assert_eq!(credentials.api_key, "test_api_key");
    }

    #[test]
    fn test_new_with_invalid_pem() {
        let result = KalshiCredentials::new("test_api_key".to_string(), "invalid pem");
        assert!(result.is_err());
        match result {
            Err(AuthError::PrivateKeyParse(_)) => {}
            _ => panic!("Expected PrivateKeyParse error"),
        }
    }

    #[test]
    fn test_sign_websocket_request() {
        let key = generate_test_key();
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let credentials = KalshiCredentials::new("test_api_key".to_string(), pem.as_str()).unwrap();

        let result = credentials.sign_websocket_request();
        assert!(result.is_ok());

        let (timestamp, signature) = result.unwrap();

        // Verify timestamp is a valid number
        assert!(timestamp.parse::<i64>().is_ok());

        // Verify signature is valid base64
        assert!(BASE64.decode(&signature).is_ok());

        // Verify signature is not empty
        assert!(!signature.is_empty());
    }

    #[test]
    fn test_debug_redacts_private_key() {
        let key = generate_test_key();
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let credentials = KalshiCredentials::new("test_api_key".to_string(), pem.as_str()).unwrap();

        let debug_output = format!("{:?}", credentials);

        // Verify api_key is present
        assert!(debug_output.contains("test_api_key"));

        // Verify private key is redacted
        assert!(debug_output.contains("<redacted>"));

        // Verify actual key material is NOT present
        assert!(!debug_output.contains("BEGIN"));
        assert!(!debug_output.contains("PRIVATE KEY"));
    }
}
