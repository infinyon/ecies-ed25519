//! ECIES-ed25519: An Integrated Encryption Scheme on Twisted Edwards Curve25519.
//!

//! ECIES can be used to encrypt data using a public key such that it can only be decrypted
//! by the holder of the corresponding private key. It is based on [curve25519-dalek](https://docs.rs/curve25519-dalek).
//!

//! ## Example Usage
//! ```rust
//! let mut csprng = rand::rng();
//! let (secret, public) = ecies_ed25519::generate_keypair(&mut csprng);
//!
//! let message = "I 💖🔒";
//!
//! // Encrypt the message with the public key such that only the holder of the secret key can decrypt.
//! let encrypted = ecies_ed25519::encrypt(&public, message.as_bytes(), &mut csprng).unwrap();
//!
//! // Decrypt the message with the secret key
//! let decrypted = ecies_ed25519::decrypt(&secret, &encrypted);
//!```
//!
//! ## `serde` support
//!
//! The `serde` feature is provided for serializing / deserializing private and public keys.
//!

use curve25519_dalek::scalar::Scalar;
use rand_core::{CryptoRng, RngCore};

mod keys;
pub use keys::*;

mod encryption;
use encryption::*;

const HKDF_INFO: &[u8; 13] = b"ecies-ed25519";

const AES_IV_LENGTH: usize = 12;

type AesKey = [u8; 32];
type SharedSecret = [u8; 32];

/// Generate a keypair, ready for use in ECIES
pub fn generate_keypair<R: CryptoRng + RngCore>(rng: &mut R) -> (SecretKey, PublicKey) {
    let secret = SecretKey::generate(rng);
    let public = PublicKey::from_secret(&secret);
    (secret, public)
}

/// Encrypt a message using ECIES, it can only be decrypted by the receiver's SecretKey.
pub fn encrypt<R: CryptoRng + RngCore>(
    receiver_pub: &PublicKey,
    msg: &[u8],
    rng: &mut R,
) -> Result<Vec<u8>, Error> {
    let (ephemeral_sk, ephemeral_pk) = generate_keypair(rng);

    let aes_key = encapsulate(&ephemeral_sk, receiver_pub);
    let encrypted = aes_encrypt(&aes_key, msg, rng)?;

    let mut cipher_text = Vec::with_capacity(PUBLIC_KEY_LENGTH + encrypted.len());
    cipher_text.extend(ephemeral_pk.to_bytes().iter());
    cipher_text.extend(encrypted);

    Ok(cipher_text)
}

/// Decrypt a ECIES encrypted ciphertext using the receiver's SecretKey.
pub fn decrypt(receiver_sec: &SecretKey, ciphertext: &[u8]) -> Result<Vec<u8>, Error> {
    if ciphertext.len() <= PUBLIC_KEY_LENGTH {
        return Err(Error::DecryptionFailedCiphertextShort);
    }

    let ephemeral_pk = PublicKey::from_bytes(&ciphertext[..PUBLIC_KEY_LENGTH])?;
    let encrypted = &ciphertext[PUBLIC_KEY_LENGTH..];
    let aes_key = decapsulate(receiver_sec, &ephemeral_pk);

    let decrypted = aes_decrypt(&aes_key, encrypted).map_err(|_| Error::DecryptionFailed)?;

    Ok(decrypted)
}

fn generate_shared(secret: &SecretKey, public: &PublicKey) -> SharedSecret {
    let public = public.to_point();
    let secret = Scalar::from_bytes_mod_order(secret.to_bytes());
    let shared_point = public * secret;
    let shared_point_compressed = shared_point.compress();

    let output = shared_point_compressed.as_bytes().to_owned();

    output
}

fn encapsulate(emphemeral_sk: &SecretKey, peer_pk: &PublicKey) -> AesKey {
    let shared_point = generate_shared(emphemeral_sk, peer_pk);

    let emphemeral_pk = PublicKey::from_secret(emphemeral_sk);

    let mut master = [0u8; 32 * 2];
    master[..32].clone_from_slice(emphemeral_pk.0.as_bytes());
    master[32..].clone_from_slice(&shared_point);

    hkdf_sha256(&master)
}

fn decapsulate(sk: &SecretKey, emphemeral_pk: &PublicKey) -> AesKey {
    let shared_point = generate_shared(sk, emphemeral_pk);

    let mut master = [0u8; 32 * 2];
    master[..32].clone_from_slice(emphemeral_pk.0.as_bytes());
    master[32..].clone_from_slice(&shared_point);

    hkdf_sha256(&master)
}

/// Error types
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// Encryption failed
    #[error("ecies-rd25519: encryption failed")]
    EncryptionFailed,

    /// Encryption failed - RNG error
    #[error("ecies-rd25519: encryption failed - RNG error")]
    EncryptionFailedRng,

    /// Decryption failed
    #[error("ecies-rd25519: decryption failed")]
    DecryptionFailed,

    /// Decryption failed - ciphertext too short
    #[error("ecies-rd25519: decryption failed - ciphertext too short")]
    DecryptionFailedCiphertextShort,

    /// Invalid public key bytes
    #[error("ecies-rd25519: invalid public key bytes")]
    InvalidPublicKeyBytes,

    /// Invalid secret key bytes
    #[error("ecies-rd25519: invalid secret key bytes")]
    InvalidSecretKeyBytes,
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use rand_core::SeedableRng;

    #[test]
    fn test_shared() {
        let (emphemeral_sk, emphemeral_pk) = generate_keypair(&mut rand::rng());
        let (peer_sk, peer_pk) = generate_keypair(&mut rand::rng());

        assert_eq!(
            generate_shared(&emphemeral_sk, &peer_pk),
            generate_shared(&peer_sk, &emphemeral_pk)
        );

        // Make sure it fails when wrong keys used
        assert_ne!(
            generate_shared(&emphemeral_sk, &emphemeral_pk),
            generate_shared(&peer_sk, &peer_pk)
        )
    }

    #[test]
    fn test_encapsulation() {
        let (emphemeral_sk, emphemeral_pk) = generate_keypair(&mut rand::rng());
        let (peer_sk, peer_pk) = generate_keypair(&mut rand::rng());

        assert_eq!(
            encapsulate(&emphemeral_sk, &peer_pk),
            decapsulate(&peer_sk, &emphemeral_pk)
        )
    }

    #[test]
    fn test_aes() {
        let mut test_rng = rand::rngs::StdRng::from_seed([0u8; 32]);
        let mut key = [0u8; 32];
        test_rng.fill_bytes(&mut key);

        let plaintext = b"ABC";
        let encrypted = aes_encrypt(&key, plaintext, &mut test_rng).unwrap();
        let decrypted = aes_decrypt(&key, &encrypted).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());

        // Test bad ciphertext
        assert!(aes_decrypt(&key, &[0u8; 16]).is_err());

        // Test bad secret key
        let bad_secret = SecretKey::generate(&mut rand::rng());
        assert!(aes_decrypt(bad_secret.as_bytes(), &encrypted).is_err());
    }

    #[test]
    fn test_ecies_ed25519() {
        let (peer_sk, peer_pk) = generate_keypair(&mut rand::rng());

        let plaintext = b"ABOLISH ICE";

        let encrypted = encrypt(&peer_pk, plaintext, &mut rand::rng()).unwrap();
        let decrypted = decrypt(&peer_sk, &encrypted).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());

        // Test bad ciphertext
        assert!(decrypt(&peer_sk, &[0u8; 16]).is_err());

        // Test that it fails when using a bad secret key
        let bad_secret = SecretKey::generate(&mut rand::rng());
        assert!(decrypt(&bad_secret, &encrypted).is_err());
    }

    #[test]
    fn test_hkdf_sha256_interop() {
        let known_key: Vec<u8> = vec![
            204, 68, 78, 7, 8, 70, 53, 136, 56, 115, 129, 183, 226, 82, 147, 253, 62, 59, 170, 188,
            131, 119, 31, 21, 249, 255, 19, 103, 230, 24, 213, 204,
        ];
        let key = hkdf_sha256(b"ABC123");

        assert_eq!(key.to_vec(), known_key);
    }

    #[test]
    fn test_aes_interop() {
        let mut test_rng = rand::rngs::StdRng::from_seed([0u8; 32]);

        let mut key = [0u8; 32];
        test_rng.fill_bytes(&mut key);

        let plaintext = b"ABC";

        let known_encrypted: Vec<u8> = vec![
            5, 100, 248, 121, 210, 122, 227, 192, 44, 232, 40, 52, 1, 159, 252, 4, 231, 249, 123,
            95, 239, 67, 162, 34, 230, 26, 163, 3, 105, 29, 35,
        ];

        let decrypted = aes_decrypt(&key, &known_encrypted).unwrap();
        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_ecies_ed25519_interop() {
        let mut test_rng = rand::rngs::StdRng::from_seed([0u8; 32]);

        let (peer_sk, _peer_pk) = generate_keypair(&mut test_rng);

        let plaintext = b"ABC";
        let known_encrypted: Vec<u8> = vec![
            2, 42, 193, 22, 163, 69, 69, 207, 149, 2, 90, 153, 255, 100, 225, 126, 108, 129, 31,
            185, 136, 165, 1, 119, 189, 186, 234, 104, 142, 53, 197, 141, 11, 213, 136, 65, 32, 62,
            116, 254, 134, 252, 113, 51, 4, 182, 127, 158, 220, 108, 210, 59, 91, 4, 222, 21, 220,
            141, 248, 54, 45, 182, 67,
        ];

        let decrypted = decrypt(&peer_sk, &known_encrypted).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_public_key_extract() {
        let mut test_rng = rand::rngs::StdRng::from_seed([0u8; 32]);

        let secret = SecretKey::generate(&mut test_rng);
        let public = PublicKey::from_secret(&secret);

        PublicKey::from_bytes(public.as_bytes()).unwrap();

        // Test bad bytes
        assert!(PublicKey::from_bytes(&[0u8; 16]).is_err());
        assert!(SecretKey::from_bytes(&[0u8; 16]).is_err());
    }

    #[test]
    fn test_hex() {
        use hex::{FromHex, ToHex};

        let mut test_rng = rand::rngs::StdRng::from_seed([0u8; 32]);
        let (secret, public) = generate_keypair(&mut test_rng);

        // lower
        let serialized_secret: String = secret.encode_hex();
        let serialized_public: String = public.encode_hex();

        let deserialized_secret = SecretKey::from_hex(serialized_secret).unwrap();
        let deserialized_public = PublicKey::from_hex(serialized_public).unwrap();

        assert_eq!(secret.to_bytes(), deserialized_secret.to_bytes());
        assert_eq!(public.as_bytes(), deserialized_public.as_bytes());

        // UPPER
        let serialized_secret: String = secret.encode_hex_upper();
        let serialized_public: String = public.encode_hex_upper();

        let deserialized_secret = SecretKey::from_hex(serialized_secret).unwrap();
        let deserialized_public = PublicKey::from_hex(serialized_public).unwrap();

        assert_eq!(secret.to_bytes(), deserialized_secret.to_bytes());
        assert_eq!(public.as_bytes(), deserialized_public.as_bytes());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_json() {
        let mut test_rng = rand::rngs::StdRng::from_seed([0u8; 32]);
        let (secret, public) = generate_keypair(&mut test_rng);

        // String
        let serialized_secret = serde_json::to_string(&secret).unwrap();
        let serialized_public = serde_json::to_string(&public).unwrap();

        let deserialized_secret: SecretKey = serde_json::from_str(&serialized_secret).unwrap();
        let deserialized_public: PublicKey = serde_json::from_str(&serialized_public).unwrap();

        assert_eq!(secret.to_bytes(), deserialized_secret.to_bytes());
        assert_eq!(public.as_bytes(), deserialized_public.as_bytes());

        // Stringy bytes
        let deserialized_secret: SecretKey =
            serde_json::from_slice(serialized_secret.as_bytes()).unwrap();
        let deserialized_public: PublicKey =
            serde_json::from_slice(serialized_public.as_bytes()).unwrap();

        assert_eq!(secret.as_bytes(), deserialized_secret.as_bytes());
        assert_eq!(public.as_bytes(), deserialized_public.as_bytes());

        let serialized_secret = serde_json::to_vec(&secret).unwrap();
        let serialized_public = serde_json::to_vec(&public).unwrap();

        let deserialized_secret: SecretKey = serde_json::from_slice(&serialized_secret).unwrap();
        let deserialized_public: PublicKey = serde_json::from_slice(&serialized_public).unwrap();

        assert_eq!(secret.as_bytes(), deserialized_secret.as_bytes());
        assert_eq!(public.as_bytes(), deserialized_public.as_bytes());

        // Test errors - mangle some bits and confirm it doesn't work:
        let mut serialized_public = serde_json::to_vec(&public).unwrap();
        serialized_public[0] = 50;
        assert!(serde_json::from_slice::<PublicKey>(&serialized_public).is_err());

        let mut serialized_public = serde_json::to_vec(&public).unwrap();
        serialized_public.push(48);
        serialized_public.push(49);
        assert!(serde_json::from_slice::<PublicKey>(&serialized_public).is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_cbor() {
        let mut test_rng = rand::rngs::StdRng::from_seed([0u8; 32]);
        let (secret, public) = generate_keypair(&mut test_rng);

        let serialized_secret = serde_cbor::to_vec(&secret).unwrap();
        let serialized_public = serde_cbor::to_vec(&public).unwrap();

        let deserialized_secret: SecretKey = serde_cbor::from_slice(&serialized_secret).unwrap();
        let deserialized_public: PublicKey = serde_cbor::from_slice(&serialized_public).unwrap();

        assert_eq!(secret.as_bytes(), deserialized_secret.as_bytes());
        assert_eq!(public.as_bytes(), deserialized_public.as_bytes());

        // Test errors - mangle some bits and confirm it doesn't work:
        let mut serialized_public = serde_cbor::to_vec(&public).unwrap();
        serialized_public[6] = 120;
        assert!(serde_cbor::from_slice::<PublicKey>(&serialized_public).is_err());
    }
}
