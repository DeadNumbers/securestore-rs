#![feature(nll)]
mod errors;
mod shared;
#[cfg(test)]
mod tests;

use self::shared::{Keys, Vault};
use crate::errors::Error;
use openssl::rand;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Used to specify where encryption/decryption keys should be loaded from
pub enum KeySource<'a> {
    /// Load the keys from a binary file on-disk
    File(&'a Path),
    /// Derive keys from the specified password
    Password(&'a str),
    /// Generate new keys from a secure RNG
    Generate,
}

/// The primary interface used for interacting with the SecureStore.
pub struct SecretsManager {
    vault: Vault,
    path: PathBuf,
    keys: Keys,
}

impl SecretsManager {
    /// Creates a new vault on-disk at path `p` and loads it in a new instance
    /// of `SecretsManager`.
    pub fn new<P: AsRef<Path>>(path: P, key_source: KeySource) -> Result<Self, Error> {
        let path = path.as_ref();

        let vault = Vault::new();
        Ok(SecretsManager {
            keys: key_source.extract_keys(&vault.iv)?,
            path: PathBuf::from(path),
            vault,
        })
    }

    /// Creates a new instance of `SecretsManager` referencing an existing vault
    /// located on-disk.
    pub fn load<P: AsRef<Path>>(path: P, key_source: KeySource) -> Result<Self, Error> {
        let path = path.as_ref();

        let vault = Vault::from_file(path)?;
        Ok(SecretsManager {
            keys: key_source.extract_keys(&vault.iv)?,
            path: PathBuf::from(path),
            vault,
        })
    }

    /// Saves changes to the underlying vault specified by the path supplied during
    /// construction of this `SecretsManager` instance.
    pub fn save(&self) -> Result<(), Error> {
        self.vault.save(&self.path)
    }

    /// Exports the private key(s) resident in memory to a path on-disk. Note that
    /// in addition to be used to export (existing) keys previously loaded into the
    /// secrets store and (new) keys generated by the secrets store, it can also be
    /// used to export keys (possibly interactively) derived from passwords to an
    /// equivalent representation on-disk.
    pub fn export_keyfile<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        self.keys.export(path)
    }
}

impl<'a> KeySource<'a> {
    fn extract_keys(&self, iv: &Option<[u8; shared::IV_SIZE]>) -> Result<Keys, Error> {
        match &self {
            KeySource::Generate => {
                let mut buffer = [0u8; shared::KEY_COUNT * shared::KEY_LENGTH];
                rand::rand_bytes(&mut buffer).expect("Key generation failure!");

                Keys::import(&buffer[..])
            }
            KeySource::File(path) => {
                match std::fs::metadata(path) {
                    Err(e) => return Err(Error::Io(e)),
                    Ok(attr) => {
                        if attr.len() as usize != shared::KEY_COUNT * shared::KEY_LENGTH {
                            return Err(Error::InvalidKeyfile);
                        }
                    }
                };

                let file = File::open(path).map_err(Error::Io)?;
                Keys::import(&file)
            }
            KeySource::Password(password) => {
                use openssl::hash::MessageDigest;
                use openssl::pkcs5::pbkdf2_hmac;

                let iv = match iv {
                    None => return Err(Error::MissingVaultIV),
                    Some(x) => x,
                };

                let mut key_data = [0u8; shared::KEY_COUNT * shared::KEY_LENGTH];
                pbkdf2_hmac(
                    password.as_bytes(),
                    iv,
                    shared::PBKDF2_ROUNDS,
                    MessageDigest::sha1(),
                    &mut key_data,
                )
                .expect("PBKDF2 key generation failed!");

                Keys::import(&key_data[..])
            }
        }
    }
}
