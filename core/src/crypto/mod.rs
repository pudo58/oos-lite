pub mod vault;

pub use vault::{write_vault_file_atomic, VaultKey, VAULT_FILE_SIZE, VAULT_MAGIC, VAULT_VERSION};
