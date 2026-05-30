//! End-to-end encryption using ECDH key exchange + AES-256-GCM.
//!
//! When `--encrypt` is passed to `filedrop receive`, an ephemeral ECDH
//! keypair is generated and the public key is embedded in the QR URL.
//! The phone extracts the public key, generates its own keypair via
//! WebCrypto, and both sides derive a shared AES-256-GCM session key.
//!
//! All file chunks are then encrypted/decrypted transparently.
//! SHA-256 verification happens on the *decrypted* data.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use hkdf::Hkdf;
use p256::{
    ecdh::EphemeralSecret,
    PublicKey,
};
use sha2::Sha256;
use std::sync::atomic::{AtomicU64, Ordering};

/// An ephemeral ECDH P-256 keypair for key exchange.
///
/// The secret is consumed when deriving the shared session key.
pub struct EphemeralKeyPair {
    secret: Option<EphemeralSecret>,
    public_key: PublicKey,
}

impl EphemeralKeyPair {
    /// Generate a new random ECDH keypair.
    pub fn generate() -> Self {
        let secret = EphemeralSecret::random(&mut rand::thread_rng());
        let public_key = secret.public_key();
        Self {
            secret: Some(secret),
            public_key,
        }
    }

    /// Get the public key as SEC1-encoded uncompressed bytes.
    pub fn public_key_bytes(&self) -> Vec<u8> {
        use p256::elliptic_curve::sec1::ToEncodedPoint;
        self.public_key.to_encoded_point(false).as_bytes().to_vec()
    }

    /// Get the public key as a base64-encoded string (for embedding in URLs).
    pub fn public_key_base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.public_key_bytes())
    }

    /// Consume the keypair and derive a session key using the peer's public key.
    ///
    /// `their_public_bytes` must be SEC1-encoded (uncompressed or compressed).
    pub fn derive_session_key(mut self, their_public_bytes: &[u8]) -> Result<SessionKey> {
        let secret = self.secret.take().context("Secret already consumed")?;

        let their_public = PublicKey::from_sec1_bytes(their_public_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid peer public key: {}", e))?;

        // Perform ECDH to get shared secret
        let shared_secret = secret.diffie_hellman(&their_public);

        // Derive AES-256 key using HKDF-SHA256
        let hkdf = Hkdf::<Sha256>::new(None, shared_secret.raw_secret_bytes().as_slice());
        let mut key_bytes = [0u8; 32];
        hkdf.expand(b"filedrop-e2e-v1", &mut key_bytes)
            .map_err(|e| anyhow::anyhow!("HKDF expansion failed: {}", e))?;

        Ok(SessionKey {
            key: key_bytes,
            nonce_counter: AtomicU64::new(0),
        })
    }
}

/// A derived AES-256-GCM session key with automatic nonce management.
///
/// Nonces are 12 bytes: first 4 bytes are zero, last 8 bytes are a
/// big-endian counter that auto-increments on each encrypt call.
pub struct SessionKey {
    key: [u8; 32],
    nonce_counter: AtomicU64,
}

impl SessionKey {
    /// Encrypt a plaintext chunk.
    ///
    /// Returns: `[12-byte nonce][ciphertext + 16-byte auth tag]`
    pub fn encrypt_chunk(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow::anyhow!("Cipher init failed: {}", e))?;

        // Build nonce: 4 zero bytes + 8 counter bytes (big-endian)
        let counter = self.nonce_counter.fetch_add(1, Ordering::SeqCst);
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..12].copy_from_slice(&counter.to_be_bytes());
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        // Prepend nonce to ciphertext
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    /// Decrypt a ciphertext chunk.
    ///
    /// Input format: `[12-byte nonce][ciphertext + 16-byte auth tag]`
    pub fn decrypt_chunk(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 + 16 {
            bail!("Encrypted data too short (need at least 28 bytes, got {})", data.len());
        }

        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow::anyhow!("Cipher init failed: {}", e))?;

        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

        Ok(plaintext)
    }
}

/// Parse a base64-encoded public key back to raw bytes.
pub fn public_key_from_base64(b64: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64)
        .context("Invalid base64 public key")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let kp = EphemeralKeyPair::generate();
        let bytes = kp.public_key_bytes();
        // Uncompressed SEC1 P-256 public key is 65 bytes (0x04 + 32 + 32)
        assert_eq!(bytes.len(), 65);
        assert_eq!(bytes[0], 0x04);
    }

    #[test]
    fn test_base64_roundtrip() {
        let kp = EphemeralKeyPair::generate();
        let b64 = kp.public_key_base64();
        let decoded = public_key_from_base64(&b64).unwrap();
        assert_eq!(decoded, kp.public_key_bytes());
    }

    #[test]
    fn test_key_exchange_and_encryption() {
        // Simulate two parties
        let alice = EphemeralKeyPair::generate();
        let bob = EphemeralKeyPair::generate();

        let alice_pub = alice.public_key_bytes();
        let bob_pub = bob.public_key_bytes();

        let alice_key = alice.derive_session_key(&bob_pub).unwrap();
        let bob_key = bob.derive_session_key(&alice_pub).unwrap();

        // Both should derive the same key
        assert_eq!(alice_key.key, bob_key.key);

        // Test encrypt/decrypt roundtrip
        let plaintext = b"Hello, FileDrop E2E!";
        let encrypted = alice_key.encrypt_chunk(plaintext).unwrap();
        let decrypted = bob_key.decrypt_chunk(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_nonce_increments() {
        let kp1 = EphemeralKeyPair::generate();
        let kp2 = EphemeralKeyPair::generate();
        let key = kp1.derive_session_key(&kp2.public_key_bytes()).unwrap();

        let enc1 = key.encrypt_chunk(b"first").unwrap();
        let enc2 = key.encrypt_chunk(b"second").unwrap();

        // Nonces should differ (first 12 bytes)
        assert_ne!(&enc1[..12], &enc2[..12]);
    }

    #[test]
    fn test_decrypt_short_data_fails() {
        let kp1 = EphemeralKeyPair::generate();
        let kp2 = EphemeralKeyPair::generate();
        let key = kp1.derive_session_key(&kp2.public_key_bytes()).unwrap();

        let result = key.decrypt_chunk(&[0u8; 10]);
        assert!(result.is_err());
    }
}
