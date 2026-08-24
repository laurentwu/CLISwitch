use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::AppResult;

pub fn bytes_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub async fn file_digest(path: &Path) -> AppResult<Option<String>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes_digest(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_stable_and_prefixed() {
        assert_eq!(
            bytes_digest(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
