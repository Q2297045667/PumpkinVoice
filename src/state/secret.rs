use aes_gcm::{
    Aes128Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use rand::Rng;
use uuid::Uuid;

#[derive(Clone)]
pub struct Secret {
    pub uuid: Uuid,
    // The cipher is stored pre-initialized to avoid repeated key expansion on every
    // encrypt/decrypt call. Secret is always cloned before being shared across tasks,
    // so each task operates on its own cipher instance (no shared mutable state).
    cipher: Aes128Gcm,
}

impl Secret {
    pub fn generate() -> Self {
        let uuid = Uuid::new_v4();
        let key = aes_gcm::Key::<Aes128Gcm>::from(*uuid.as_bytes());
        let cipher = Aes128Gcm::new(&key);
        Secret { uuid, cipher }
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        *self.uuid.as_bytes()
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        let uuid = Uuid::from_bytes(bytes);
        let key = aes_gcm::Key::<Aes128Gcm>::from(bytes);
        let cipher = Aes128Gcm::new(&key);
        Secret { uuid, cipher }
    }

    pub fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, aes_gcm::Error> {
        let mut iv = [0u8; 12];
        rand::rng().fill_bytes(&mut iv);
        let nonce = Nonce::from(iv);

        let enc = self.cipher.encrypt(&nonce, data)?;

        let mut payload = Vec::with_capacity(iv.len() + enc.len());
        payload.extend_from_slice(&iv);
        payload.extend_from_slice(&enc);

        Ok(payload)
    }

    pub fn decrypt(&self, payload: &[u8]) -> Result<Vec<u8>, aes_gcm::Error> {
        if payload.len() < 12 {
            return Err(aes_gcm::Error);
        }

        let nonce = Nonce::try_from(&payload[0..12]).map_err(|_| aes_gcm::Error)?;
        let data = &payload[12..];

        self.cipher.decrypt(&nonce, data)
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn encrypted_payload_round_trips() {
        let secret = Secret::generate();
        let plaintext = b"pumpkin voice api migration";

        let encrypted = secret
            .encrypt(plaintext)
            .expect("encryption should succeed");

        assert_ne!(encrypted, plaintext);
        assert_eq!(
            secret
                .decrypt(&encrypted)
                .expect("decryption should succeed"),
            plaintext
        );
    }

    #[test]
    fn rejects_payload_without_a_full_nonce() {
        let secret = Secret::generate();

        assert!(secret.decrypt(&[0; 11]).is_err());
    }
}
