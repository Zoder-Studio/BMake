use anyhow::{bail, Context, Result};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

const MAGIC: &[u8; 6] = b"BMKVLT";
const FORMAT_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

#[derive(Debug, Serialize, Deserialize, Default)]
struct VaultPayload {
    vault_id: String,
    created_at: u64,
    modified_at: u64,
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

/// A `secret.bm.locksys` vault: ChaCha20-Poly1305 AEAD, key derived from a
/// user passphrase via Argon2id with a random per-vault salt. The magic +
/// format version + salt + nonce are stored in plaintext (none of that is
/// secret); the AEAD authentication tag means any corruption or tampering
/// makes decryption fail outright — that IS the integrity check, no
/// separate checksum needed.
pub struct Vault {
    path: PathBuf,
}

impl Vault {
    pub fn at(project_dir: &Path) -> Self {
        Self { path: project_dir.join("secret.bm.locksys") }
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn create(&self, passphrase: &str) -> Result<()> {
        if self.path.exists() {
            bail!("secret.bm.locksys already exists");
        }
        let now = now_unix();
        let payload = VaultPayload {
            vault_id: new_vault_id(),
            created_at: now,
            modified_at: now,
            secrets: BTreeMap::new(),
        };
        self.write_payload(passphrase, &payload)
    }

    pub fn add_secret(&self, passphrase: &str, name: &str, value: &str) -> Result<()> {
        validate_secret_name(name)?;
        let mut payload = self.read_payload(passphrase)?;
        payload.secrets.insert(name.to_string(), value.to_string());
        payload.modified_at = now_unix();
        self.write_payload(passphrase, &payload)
    }

    pub fn remove_secret(&self, passphrase: &str, name: &str) -> Result<bool> {
        let mut payload = self.read_payload(passphrase)?;
        let removed = payload.secrets.remove(name).is_some();
        if removed {
            payload.modified_at = now_unix();
            self.write_payload(passphrase, &payload)?;
        }
        Ok(removed)
    }

    pub fn list_names(&self, passphrase: &str) -> Result<Vec<String>> {
        let payload = self.read_payload(passphrase)?;
        Ok(payload.secrets.keys().cloned().collect())
    }

    /// Decrypts the vault once, but only pulls out the specifically
    /// requested secret names — the rest of the decrypted payload is
    /// dropped (and zeroized) immediately after this call returns.
    pub fn get_secrets(&self, passphrase: &str, names: &[String]) -> Result<HashMap<String, String>> {
        let payload = self.read_payload(passphrase)?;
        let mut out = HashMap::new();
        for name in names {
            let Some(value) = payload.secrets.get(name) else {
                bail!(
                    "Secret \"{}\" was not found.\n\nCreate it with:\n\n    bmake add secret {}",
                    name,
                    name
                );
            };
            out.insert(name.clone(), value.clone());
        }
        Ok(out)
    }

    fn read_payload(&self, passphrase: &str) -> Result<VaultPayload> {
        let bytes = std::fs::read(&self.path).with_context(|| format!("Failed to read {}", self.path.display()))?;
        if bytes.len() < MAGIC.len() + 1 + SALT_LEN + NONCE_LEN {
            bail!("Secret vault integrity check failed.\n\nThe secret.bm.locksys file may be corrupted or modified.");
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            bail!("Secret vault integrity check failed.\n\nThe secret.bm.locksys file may be corrupted or modified.");
        }
        let version = bytes[MAGIC.len()];
        if version != FORMAT_VERSION {
            bail!(
                "secret.bm.locksys was created by an incompatible vault format (version {}). This build supports version {}.",
                version,
                FORMAT_VERSION
            );
        }

        let header_len = MAGIC.len() + 1;
        let salt = &bytes[header_len..header_len + SALT_LEN];
        let nonce_bytes = &bytes[header_len + SALT_LEN..header_len + SALT_LEN + NONCE_LEN];
        let ciphertext = &bytes[header_len + SALT_LEN + NONCE_LEN..];
        let header = &bytes[..header_len + SALT_LEN + NONCE_LEN];

        let key = derive_key(passphrase, salt)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&*key));
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, Payload { msg: ciphertext, aad: header })
            .map_err(|_| {
                anyhow::anyhow!(
                    "Secret vault integrity check failed.\n\nThe secret.bm.locksys file may be corrupted or modified, or the passphrase is wrong."
                )
            })?;
        let plaintext = Zeroizing::new(plaintext);

        let text = std::str::from_utf8(&plaintext)
            .map_err(|_| anyhow::anyhow!("Secret vault integrity check failed.\n\nThe secret.bm.locksys file may be corrupted or modified."))?;
        let payload: VaultPayload = toml::from_str(text)
            .map_err(|_| anyhow::anyhow!("Secret vault integrity check failed.\n\nThe secret.bm.locksys file may be corrupted or modified."))?;
        Ok(payload)
    }

    fn write_payload(&self, passphrase: &str, payload: &VaultPayload) -> Result<()> {
        let plaintext = Zeroizing::new(toml::to_string(payload)?.into_bytes());

        let mut salt = [0u8; SALT_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

        let key = derive_key(passphrase, &salt)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&*key));
        let nonce = Nonce::from_slice(&nonce_bytes);

        let mut header = Vec::with_capacity(MAGIC.len() + 1 + SALT_LEN + NONCE_LEN);
        header.extend_from_slice(MAGIC);
        header.push(FORMAT_VERSION);
        header.extend_from_slice(&salt);
        header.extend_from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, Payload { msg: &plaintext, aad: &header })
            .map_err(|_| anyhow::anyhow!("Failed to encrypt secret vault"))?;

        let mut out = header;
        out.extend_from_slice(&ciphertext);

        let tmp = self.path.with_extension("locksys.tmp");
        std::fs::write(&tmp, &out)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
        .map_err(|e| anyhow::anyhow!("Key derivation failed: {}", e))?;
    Ok(key)
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn new_vault_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Rejects path traversal, control characters, and empty names — but not
/// any particular prefix; per spec, any valid name is allowed.
pub fn validate_secret_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Secret name cannot be empty");
    }
    if name == "." || name == ".." {
        bail!("Secret name '{}' is not allowed", name);
    }
    if name.contains(['/', '\\', '\0']) || name.chars().any(|c| c.is_control()) {
        bail!("Secret name '{}' contains invalid characters", name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_add_and_get_secret() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::at(dir.path());
        vault.create("correct horse battery staple").unwrap();
        vault.add_secret("correct horse battery staple", "DeployToken", "s3cr3t-value").unwrap();

        let got = vault.get_secrets("correct horse battery staple", &["DeployToken".to_string()]).unwrap();
        assert_eq!(got.get("DeployToken").unwrap(), "s3cr3t-value");
    }

    #[test]
    fn wrong_passphrase_fails_integrity_check() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::at(dir.path());
        vault.create("right-passphrase").unwrap();
        vault.add_secret("right-passphrase", "X", "y").unwrap();

        let err = vault.list_names("wrong-passphrase").unwrap_err();
        assert!(err.to_string().contains("integrity check failed"));
    }

    #[test]
    fn tampered_vault_fails_integrity_check() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::at(dir.path());
        vault.create("passphrase").unwrap();
        vault.add_secret("passphrase", "X", "y").unwrap();

        let path = dir.path().join("secret.bm.locksys");
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(&path, bytes).unwrap();

        let err = vault.list_names("passphrase").unwrap_err();
        assert!(err.to_string().contains("integrity check failed"));
    }

    #[test]
    fn remove_secret_reports_whether_it_existed() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::at(dir.path());
        vault.create("p").unwrap();
        vault.add_secret("p", "X", "y").unwrap();

        assert!(vault.remove_secret("p", "X").unwrap());
        assert!(!vault.remove_secret("p", "X").unwrap());
    }

    #[test]
    fn secret_name_validation_rejects_path_traversal() {
        assert!(validate_secret_name("../etc/passwd").is_err());
        assert!(validate_secret_name("a/b").is_err());
        assert!(validate_secret_name("DeployToken").is_ok());
        assert!(validate_secret_name("banana").is_ok());
    }
}