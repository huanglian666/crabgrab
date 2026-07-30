use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("cannot stage MediaInfo report in {path}: {source}")]
    Stage { path: PathBuf, source: io::Error },
    #[error("cannot replace MediaInfo report {path}: {source}")]
    Replace { path: PathBuf, source: io::Error },
}

pub(crate) fn install_report(report: &str, output: &Path) -> Result<PathBuf, InstallError> {
    let target = output.join("mediainfo.txt");
    let normalized = format!(
        "{}\n",
        report.replace("\r\n", "\n").replace('\r', "\n").trim_end()
    );
    let mut staged = NamedTempFile::new_in(output).map_err(|source| InstallError::Stage {
        path: output.to_path_buf(),
        source,
    })?;
    staged
        .write_all(normalized.as_bytes())
        .and_then(|()| staged.flush())
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|source| InstallError::Stage {
            path: staged.path().to_path_buf(),
            source,
        })?;

    let backup = NamedTempFile::new_in(output).map_err(|source| InstallError::Stage {
        path: output.to_path_buf(),
        source,
    })?;
    let backup_path = backup.path().to_path_buf();
    drop(backup);
    let had_target = target.exists();
    if had_target {
        fs::rename(&target, &backup_path).map_err(|source| InstallError::Replace {
            path: target.clone(),
            source,
        })?;
    }

    match staged.persist(&target) {
        Ok(_) => {
            if had_target {
                let _ = fs::remove_file(backup_path);
            }
            Ok(target)
        }
        Err(error) => {
            if had_target {
                let _ = fs::rename(&backup_path, &target);
            }
            Err(InstallError::Replace {
                path: target,
                source: error.error,
            })
        }
    }
}
