use std::fmt::Display;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::Builder;
use thiserror::Error;

const MARKER: &str = ".crabgrab-screenshots";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenshotManifest {
    pub ffmpeg_version: String,
    pub video: String,
    pub images: Vec<ManifestImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestImage {
    pub file: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("cannot prepare screenshot output {path}: {source}")]
    Prepare {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("screenshot directory is not owned by CrabGrab: {0}")]
    NotOwned(PathBuf),
    #[error("cannot generate screenshots: {0}")]
    Generate(String),
    #[error("cannot write screenshot manifest {path}: {message}")]
    Manifest { path: PathBuf, message: String },
    #[error("cannot replace screenshot directory {path}: {source}")]
    Replace {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub fn install_screenshots<E: Display>(
    output_root: &Path,
    manifest: &ScreenshotManifest,
    generate: impl FnOnce(&Path) -> Result<(), E>,
) -> Result<PathBuf, InstallError> {
    let manifest = manifest.clone();
    install_generated_screenshots(output_root, |stage| {
        generate(stage)?;
        Ok::<_, E>(manifest)
    })
}

pub(super) fn install_generated_screenshots<E: Display>(
    output_root: &Path,
    generate: impl FnOnce(&Path) -> Result<ScreenshotManifest, E>,
) -> Result<PathBuf, InstallError> {
    prepare_output_root(output_root)?;
    let target = output_root.join("screenshots");
    validate_existing_target(&target)?;

    let stage = Builder::new()
        .prefix(".screenshots.tmp-")
        .tempdir_in(output_root)
        .map_err(|source| InstallError::Prepare {
            path: output_root.to_path_buf(),
            source,
        })?;
    let manifest =
        generate(stage.path()).map_err(|error| InstallError::Generate(error.to_string()))?;
    write_manifest(stage.path(), &manifest)?;

    let backup_path = if target.exists() {
        let slot = Builder::new()
            .prefix(".screenshots.backup-")
            .tempdir_in(output_root)
            .map_err(|source| InstallError::Prepare {
                path: output_root.to_path_buf(),
                source,
            })?;
        let backup = slot.path().to_path_buf();
        slot.close().map_err(|source| InstallError::Prepare {
            path: backup.clone(),
            source,
        })?;
        fs::rename(&target, &backup).map_err(|source| InstallError::Replace {
            path: target.clone(),
            source,
        })?;
        Some(backup)
    } else {
        None
    };

    let staged_path = stage.keep();
    if let Err(source) = fs::rename(&staged_path, &target) {
        if let Some(backup) = &backup_path {
            let _ = fs::rename(backup, &target);
        }
        let _ = fs::remove_dir_all(staged_path);
        return Err(InstallError::Replace {
            path: target,
            source,
        });
    }
    if let Some(backup) = backup_path {
        let _ = fs::remove_dir_all(backup);
    }
    Ok(target)
}

fn prepare_output_root(output_root: &Path) -> Result<(), InstallError> {
    if output_root.exists() && !output_root.is_dir() {
        return Err(InstallError::Prepare {
            path: output_root.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "path is not a directory",
            ),
        });
    }
    fs::create_dir_all(output_root).map_err(|source| InstallError::Prepare {
        path: output_root.to_path_buf(),
        source,
    })
}

fn validate_existing_target(target: &Path) -> Result<(), InstallError> {
    if !target.exists() {
        return Ok(());
    }
    if !target.is_dir() {
        return Err(InstallError::NotOwned(target.to_path_buf()));
    }
    let mut entries = fs::read_dir(target).map_err(|source| InstallError::Prepare {
        path: target.to_path_buf(),
        source,
    })?;
    if entries.next().is_none() {
        return Ok(());
    }
    let marker_path = target.join(MARKER);
    let marker = fs::read_to_string(&marker_path)
        .map_err(|_| InstallError::NotOwned(target.to_path_buf()))?;
    toml::from_str::<ScreenshotManifest>(&marker)
        .map(|_| ())
        .map_err(|_| InstallError::NotOwned(target.to_path_buf()))
}

fn write_manifest(stage: &Path, manifest: &ScreenshotManifest) -> Result<(), InstallError> {
    let path = stage.join(MARKER);
    let contents = toml::to_string_pretty(manifest).map_err(|error| InstallError::Manifest {
        path: path.clone(),
        message: error.to_string(),
    })?;
    let mut file = File::create(&path).map_err(|error| InstallError::Manifest {
        path: path.clone(),
        message: error.to_string(),
    })?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| InstallError::Manifest {
            path,
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{ManifestImage, ScreenshotManifest, install_screenshots};

    fn manifest(images: &[(&str, &str)]) -> ScreenshotManifest {
        ScreenshotManifest {
            ffmpeg_version: "6.1.1".into(),
            video: "Movie.mkv".into(),
            images: images
                .iter()
                .map(|(file, timestamp)| ManifestImage {
                    file: (*file).into(),
                    timestamp: (*timestamp).into(),
                    subtitle: None,
                })
                .collect(),
        }
    }

    #[test]
    fn installs_images_and_machine_readable_ownership_manifest() {
        let root = tempdir().unwrap();
        let output = root.path().join("result");

        let installed =
            install_screenshots(&output, &manifest(&[("01.png", "00:10:30")]), |stage| {
                fs::write(stage.join("01.png"), b"png")?;
                Ok::<_, std::io::Error>(())
            })
            .unwrap();

        assert_eq!(installed, output.join("screenshots"));
        assert_eq!(fs::read(installed.join("01.png")).unwrap(), b"png");
        let marker = fs::read_to_string(installed.join(".crabgrab-screenshots")).unwrap();
        assert!(marker.contains("timestamp = \"00:10:30\""));
        assert!(!marker.contains("subtitle ="));
    }

    #[test]
    fn refuses_nonempty_directory_without_ownership_marker() {
        let root = tempdir().unwrap();
        let screenshots = root.path().join("result/screenshots");
        fs::create_dir_all(&screenshots).unwrap();
        fs::write(screenshots.join("mine.png"), b"user").unwrap();

        let error = install_screenshots(&root.path().join("result"), &manifest(&[]), |_| {
            Ok::<_, std::io::Error>(())
        })
        .unwrap_err();

        assert!(error.to_string().contains("not owned"));
        assert_eq!(fs::read(screenshots.join("mine.png")).unwrap(), b"user");
    }

    #[test]
    fn generation_failure_preserves_previous_owned_directory() {
        let root = tempdir().unwrap();
        let output = root.path().join("result");
        install_screenshots(&output, &manifest(&[("01.png", "00:01:00")]), |stage| {
            fs::write(stage.join("01.png"), b"old")?;
            Ok::<_, std::io::Error>(())
        })
        .unwrap();

        let error = install_screenshots(&output, &manifest(&[("01.png", "00:02:00")]), |_stage| {
            Err(std::io::Error::other("decode failed"))
        })
        .unwrap_err();

        assert!(error.to_string().contains("decode failed"));
        assert_eq!(fs::read(output.join("screenshots/01.png")).unwrap(), b"old");
    }

    #[test]
    fn replacing_owned_directory_removes_stale_images() {
        let root = tempdir().unwrap();
        let output = root.path().join("result");
        install_screenshots(
            &output,
            &manifest(&[("01.png", "00:01:00"), ("02.png", "00:02:00")]),
            |stage| {
                fs::write(stage.join("01.png"), b"old one")?;
                fs::write(stage.join("02.png"), b"old two")?;
                Ok::<_, std::io::Error>(())
            },
        )
        .unwrap();

        install_screenshots(&output, &manifest(&[("01.png", "00:03:00")]), |stage| {
            fs::write(stage.join("01.png"), b"new")?;
            Ok::<_, std::io::Error>(())
        })
        .unwrap();

        assert_eq!(fs::read(output.join("screenshots/01.png")).unwrap(), b"new");
        assert!(!output.join("screenshots/02.png").exists());
    }
}
