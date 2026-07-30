use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("bundled MediaInfo is missing: {0}")]
    Missing(PathBuf),
    #[error("cannot read bundled MediaInfo {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("bundled MediaInfo SHA-256 verification failed: {0}")]
    Hash(PathBuf),
}

pub fn verify_tool(path: &Path, expected_sha256: &str) -> Result<(), ToolError> {
    if !path.is_file() {
        return Err(ToolError::Missing(path.to_path_buf()));
    }
    let actual = sha256(path).map_err(|source| ToolError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if actual != expected_sha256.to_ascii_lowercase() {
        return Err(ToolError::Hash(path.to_path_buf()));
    }
    Ok(())
}

pub(crate) fn sha256(path: &Path) -> Result<String, io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::verify_tool;

    #[test]
    fn rejects_missing_sidecar() {
        let root = tempdir().unwrap();
        let error = verify_tool(&root.path().join("mediainfo"), "unused").unwrap_err();
        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn accepts_sidecar_with_expected_sha256() {
        let root = tempdir().unwrap();
        let tool = root.path().join("mediainfo");
        fs::write(&tool, b"test").unwrap();

        verify_tool(
            &tool,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        )
        .unwrap();
    }

    #[test]
    fn rejects_sidecar_with_wrong_sha256() {
        let root = tempdir().unwrap();
        let tool = root.path().join("mediainfo");
        fs::write(&tool, b"modified").unwrap();

        let error = verify_tool(&tool, &"0".repeat(64)).unwrap_err();
        assert!(error.to_string().contains("SHA-256"));
    }
}
