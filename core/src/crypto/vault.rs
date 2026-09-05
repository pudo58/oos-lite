use std::io::Write;
use std::path::Path;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{OosLiteError, Result};

pub const VAULT_MAGIC: &[u8; 4] = b"OOSK";
pub const VAULT_VERSION: u32 = 1;
pub const VAULT_FILE_SIZE: usize = 100;

/// Secure in-memory Master Key for encrypting and decrypting chunks and WAL records.
/// Automatically zeroizes memory on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct VaultKey {
    master_key: [u8; 32],
}

impl VaultKey {
    pub fn from_raw(master_key: [u8; 32]) -> Self {
        Self { master_key }
    }

    pub fn master_key(&self) -> &[u8; 32] {
        &self.master_key
    }

    /// Derives Key Encryption Key (KEK) from passphrase and salt using Argon2id.
    fn derive_kek(passphrase: &str, salt: &[u8; 16]) -> Result<[u8; 32]> {
        let params = Params::new(
            64 * 1024, // 64 MiB
            3,         // 3 iterations
            4,         // 4 parallelism
            Some(32),
        )
        .map_err(|e| OosLiteError::Internal(format!("Invalid Argon2 params: {e}")))?;

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut kek = [0u8; 32];
        argon2
            .hash_password_into(passphrase.as_bytes(), salt, &mut kek)
            .map_err(|e| OosLiteError::Internal(format!("Argon2 key derivation failed: {e}")))?;

        Ok(kek)
    }

    /// Creates a brand new VaultKey with a cryptographically secure random 256-bit Master Key,
    /// and generates the encrypted 100-byte `vault.key` payload.
    pub fn create(passphrase: &str) -> Result<(Self, Vec<u8>)> {
        if passphrase.trim().is_empty() {
            return Err(OosLiteError::AuthenticationFailed(
                "Passphrase cannot be empty".to_string(),
            ));
        }
        if passphrase.len() < 8 {
            return Err(OosLiteError::AuthenticationFailed(
                "Passphrase is too short: minimum 8 characters required".to_string(),
            ));
        }

        let mut master_key = [0u8; 32];
        OsRng.fill_bytes(&mut master_key);

        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);

        let mut kek = Self::derive_kek(passphrase, &salt)?;

        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);

        // Header bytes up to nonce act as Associated Authenticated Data (AAD)
        let mut aad = Vec::with_capacity(24);
        aad.extend_from_slice(VAULT_MAGIC);
        aad.extend_from_slice(&VAULT_VERSION.to_le_bytes());
        aad.extend_from_slice(&salt);

        let cipher = XChaCha20Poly1305::new_from_slice(&kek)
            .map_err(|e| OosLiteError::Internal(format!("XChaCha20 init error: {e}")))?;
        kek.zeroize();

        let payload = Payload {
            msg: &master_key,
            aad: &aad,
        };

        let ciphertext_and_tag = cipher
            .encrypt(XNonce::from_slice(&nonce), payload)
            .map_err(|e| OosLiteError::Internal(format!("Master key encryption failed: {e}")))?;

        if ciphertext_and_tag.len() != 48 {
            return Err(OosLiteError::Internal(
                "Unexpected encrypted master key size".to_string(),
            ));
        }

        let mut vault_bytes = Vec::with_capacity(VAULT_FILE_SIZE);
        vault_bytes.extend_from_slice(VAULT_MAGIC);
        vault_bytes.extend_from_slice(&VAULT_VERSION.to_le_bytes());
        vault_bytes.extend_from_slice(&salt);
        vault_bytes.extend_from_slice(&nonce);
        vault_bytes.extend_from_slice(&ciphertext_and_tag); // 32 bytes cipher + 16 bytes tag = 48 bytes

        let checksum = crc32fast::hash(&vault_bytes[..96]);
        vault_bytes.extend_from_slice(&checksum.to_le_bytes());

        Ok((Self { master_key }, vault_bytes))
    }

    /// Unlocks an existing `vault.key` payload using the provided passphrase.
    pub fn unlock(passphrase: &str, bytes: &[u8]) -> Result<Self> {
        if bytes.len() != VAULT_FILE_SIZE {
            return Err(OosLiteError::AuthenticationFailed(format!(
                "Invalid vault key file size: expected {}, got {}",
                VAULT_FILE_SIZE,
                bytes.len()
            )));
        }

        if &bytes[0..4] != VAULT_MAGIC {
            return Err(OosLiteError::AuthenticationFailed(
                "Invalid vault key magic header".to_string(),
            ));
        }

        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != VAULT_VERSION {
            return Err(OosLiteError::AuthenticationFailed(format!(
                "Unsupported vault key version: {}",
                version
            )));
        }

        let expected_checksum = crc32fast::hash(&bytes[..96]);
        let stored_checksum = u32::from_le_bytes(bytes[96..100].try_into().unwrap());
        if expected_checksum != stored_checksum {
            return Err(OosLiteError::AuthenticationFailed(
                "Vault key file corrupted (checksum mismatch)".to_string(),
            ));
        }

        let mut salt = [0u8; 16];
        salt.copy_from_slice(&bytes[8..24]);

        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&bytes[24..48]);

        let ciphertext_and_tag = &bytes[48..96];

        let mut kek = Self::derive_kek(passphrase, &salt)?;

        let mut aad = Vec::with_capacity(24);
        aad.extend_from_slice(VAULT_MAGIC);
        aad.extend_from_slice(&VAULT_VERSION.to_le_bytes());
        aad.extend_from_slice(&salt);

        let cipher = XChaCha20Poly1305::new_from_slice(&kek)
            .map_err(|e| OosLiteError::Internal(format!("XChaCha20 init error: {e}")))?;
        kek.zeroize();

        let payload = Payload {
            msg: ciphertext_and_tag,
            aad: &aad,
        };

        let decrypted = cipher
            .decrypt(XNonce::from_slice(&nonce), payload)
            .map_err(|_| {
                OosLiteError::AuthenticationFailed(
                    "Incorrect passphrase or vault key data was tampered with (decryption failed)"
                        .to_string(),
                )
            })?;

        if decrypted.len() != 32 {
            return Err(OosLiteError::AuthenticationFailed(
                "Decrypted master key has invalid length".to_string(),
            ));
        }

        let mut master_key = [0u8; 32];
        master_key.copy_from_slice(&decrypted);

        Ok(Self { master_key })
    }

    /// Encrypts chunk data with XChaCha20-Poly1305 and the given nonce and Additional Authenticated Data (AAD).
    /// Returns ciphertext with appended 16-byte Poly1305 authentication tag.
    pub fn encrypt_chunk(&self, plaintext: &[u8], nonce: &[u8; 24], aad: &[u8]) -> Result<Vec<u8>> {
        let cipher = XChaCha20Poly1305::new_from_slice(&self.master_key)
            .map_err(|e| OosLiteError::Internal(format!("XChaCha20 cipher init failed: {e}")))?;

        let payload = Payload {
            msg: plaintext,
            aad,
        };

        cipher
            .encrypt(XNonce::from_slice(nonce), payload)
            .map_err(|e| OosLiteError::Internal(format!("Chunk encryption failed: {e}")))
    }

    /// Decrypts and authenticates chunk data with XChaCha20-Poly1305 and the given nonce and AAD.
    pub fn decrypt_chunk(
        &self,
        ciphertext_and_tag: &[u8],
        nonce: &[u8; 24],
        aad: &[u8],
    ) -> Result<Vec<u8>> {
        let cipher = XChaCha20Poly1305::new_from_slice(&self.master_key)
            .map_err(|e| OosLiteError::Internal(format!("XChaCha20 cipher init failed: {e}")))?;

        let payload = Payload {
            msg: ciphertext_and_tag,
            aad,
        };

        cipher
            .decrypt(XNonce::from_slice(nonce), payload)
            .map_err(|_| {
                OosLiteError::DecryptionFailed(
                    "Xác thực Poly1305 thất bại (dữ liệu chunk bị giả mạo hoặc sai khóa giải mã)"
                        .to_string(),
                )
            })
    }

    /// Saves vault key bytes to a file atomically with fsync and 0600 permissions.
    pub fn save_to_file<P: AsRef<Path>>(path: P, bytes: &[u8]) -> Result<()> {
        write_vault_file_atomic(path.as_ref(), bytes)
    }
}

/// Atomically writes a vault key payload to disk with fsync and secure file permissions (0600 on Unix).
pub fn write_vault_file_atomic(vault_path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = vault_path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = parent.join(format!(".vault.key.tmp_{}_{}", std::process::id(), now_ns));

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = file.metadata()?.permissions();
        perms.set_mode(0o600); // Read/write only by owner
        file.set_permissions(perms)?;
    }

    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    std::fs::rename(&tmp_path, vault_path)?;

    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(())
}
