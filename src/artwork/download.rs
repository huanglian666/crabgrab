use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use reqwest::Url;
use reqwest::blocking::Client;
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::providers::Artwork;

#[derive(Debug)]
pub struct DownloadedArtwork {
    pub background: PathBuf,
    pub cover: PathBuf,
}

pub trait BinaryFetcher {
    fn fetch_to(&self, url: &Url, destination: &mut File) -> Result<(), DownloadError>;
}

pub struct ReqwestBinaryFetcher {
    client: Client,
}

impl ReqwestBinaryFetcher {
    /// 使用已经配置好超时与重定向策略的客户端创建下载器。
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl BinaryFetcher for ReqwestBinaryFetcher {
    /// 将响应流直接复制到文件，避免把整张 original 图片加载到内存。
    fn fetch_to(&self, url: &Url, destination: &mut File) -> Result<(), DownloadError> {
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .map_err(|error| DownloadError::Fetch(error.to_string()))?;
        if !response.status().is_success() {
            return Err(DownloadError::Fetch(format!(
                "image server returned HTTP {}",
                response.status().as_u16()
            )));
        }
        io::copy(&mut response, destination)?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("image download failed: {0}")]
    Fetch(String),
    #[error("file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("could not preserve temporary file: {0}")]
    Persist(#[from] tempfile::PathPersistError),
    #[error("artwork replacement failed: {original}; rollback: {rollback}")]
    Rollback { original: String, rollback: String },
    #[error("artwork was installed, but backup cleanup failed: {0}")]
    CommittedCleanup(String),
}

trait FileOps {
    fn exists(&self, path: &Path) -> io::Result<bool>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove(&self, path: &Path) -> io::Result<()>;
}

struct StdFileOps;

impl FileOps for StdFileOps {
    fn exists(&self, path: &Path) -> io::Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

/// 下载并事务式安装背景图和封面。
///
/// 两张图片先完整暂存；只有两次下载都成功后才替换已有正式文件。
pub fn install_artwork(
    fetcher: &dyn BinaryFetcher,
    artwork: &Artwork,
    output_parent: &Path,
) -> Result<DownloadedArtwork, DownloadError> {
    // 先把两张图片完整暂存，任一下载失败都不会触碰已有正式文件。
    let output_parent = if output_parent.is_absolute() {
        output_parent.to_owned()
    } else {
        std::env::current_dir()?.join(output_parent)
    };
    let background_dir = output_parent.join("background");
    let cover_dir = output_parent.join("cover");
    fs::create_dir_all(&background_dir)?;
    fs::create_dir_all(&cover_dir)?;
    let background = background_dir.join("background.jpg");
    let cover = cover_dir.join("cover.jpg");

    let staged_background = stage(fetcher, &artwork.background_url, &background_dir)?;
    // 第二张暂存失败时清理第一张，清理失败也必须进入最终错误信息。
    let staged_cover = match stage(fetcher, &artwork.cover_url, &cover_dir) {
        Ok(path) => path,
        Err(error) => {
            if let Err(cleanup) = fs::remove_file(&staged_background) {
                return Err(DownloadError::Rollback {
                    original: error.to_string(),
                    rollback: format!("cannot remove {}: {cleanup}", staged_background.display()),
                });
            }
            return Err(error);
        }
    };

    replace_pair_with_ops(
        &StdFileOps,
        &staged_background,
        &background,
        &staged_cover,
        &cover,
    )?;

    Ok(DownloadedArtwork { background, cover })
}

fn stage(
    fetcher: &dyn BinaryFetcher,
    url: &Url,
    directory: &Path,
) -> Result<PathBuf, DownloadError> {
    // 临时文件放在目标目录内，保证后续重命名不会跨文件系统。
    let mut temporary = NamedTempFile::new_in(directory)?;
    fetcher.fetch_to(url, temporary.as_file_mut())?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    Ok(temporary.into_temp_path().keep()?)
}

fn reserve_backup(ops: &dyn FileOps, target: &Path) -> Result<Option<PathBuf>, io::Error> {
    // 备份与目标位于同一目录，后续重命名不会跨文件系统。
    if !ops.exists(target)? {
        return Ok(None);
    }
    let temporary = NamedTempFile::new_in(target.parent().expect("target has parent"))?;
    let backup = temporary.path().to_owned();
    temporary.close()?;
    ops.rename(target, &backup)?;
    Ok(Some(backup))
}

fn restore(ops: &dyn FileOps, backup: &Option<PathBuf>, target: &Path) -> Result<(), io::Error> {
    // 有备份就恢复旧文件；原目标不存在时则移除已经安装的新文件。
    if let Some(backup) = backup {
        if ops.exists(target)? {
            ops.remove(target)?;
        }
        ops.rename(backup, target)?;
    } else if ops.exists(target)? {
        ops.remove(target)?;
    }
    Ok(())
}

fn replace_pair_with_ops(
    ops: &dyn FileOps,
    staged_background: &Path,
    background: &Path,
    staged_cover: &Path,
    cover: &Path,
) -> Result<(), DownloadError> {
    // 两个文件无法由文件系统一次原子提交，因此按状态逆序回滚已完成的步骤。
    let background_backup = match reserve_backup(ops, background) {
        Ok(backup) => backup,
        Err(error) => {
            cleanup_paths(ops, [staged_background, staged_cover])?;
            return Err(error.into());
        }
    };
    let cover_backup = match reserve_backup(ops, cover) {
        Ok(backup) => backup,
        Err(error) => {
            // staged 清理即使失败，也不能阻止已经备份的 background 恢复。
            let cleanup = cleanup_paths(ops, [staged_background, staged_cover]);
            let rollback = restore(ops, &background_backup, background);
            if rollback.is_err() || cleanup.is_err() {
                let details = [
                    rollback.err().map(|value| format!("restore: {value}")),
                    cleanup.err().map(|value| format!("cleanup: {value}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("; ");
                return Err(DownloadError::Rollback {
                    original: error.to_string(),
                    rollback: details,
                });
            }
            return Err(error.into());
        }
    };

    let replacement = ops
        .rename(staged_background, background)
        .and_then(|_| ops.rename(staged_cover, cover));
    if let Err(error) = replacement {
        // 两个恢复动作和临时清理全部执行后，再聚合任一失败信息。
        let first = restore(ops, &background_backup, background);
        let second = restore(ops, &cover_backup, cover);
        let cleanup = cleanup_paths(ops, [staged_background, staged_cover]);
        if let Err(rollback) = first.and(second).and(cleanup) {
            return Err(DownloadError::Rollback {
                original: error.to_string(),
                rollback: rollback.to_string(),
            });
        }
        return Err(error.into());
    }

    let mut cleanup_errors = Vec::new();
    // 正式文件已经提交，两个备份都应尽力删除，不能遇到首个错误就停止。
    for backup in [background_backup, cover_backup].into_iter().flatten() {
        if let Err(error) = ops.remove(&backup) {
            cleanup_errors.push(format!("{}: {error}", backup.display()));
        }
    }
    if !cleanup_errors.is_empty() {
        return Err(DownloadError::CommittedCleanup(cleanup_errors.join("; ")));
    }
    Ok(())
}

fn cleanup_paths<'a>(
    ops: &dyn FileOps,
    paths: impl IntoIterator<Item = &'a Path>,
) -> Result<(), io::Error> {
    let mut first_error = None;
    for path in paths {
        match ops.remove(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs::{self, File};
    use std::io::Write;

    use reqwest::Url;
    use tempfile::tempdir;

    use crate::providers::Artwork;

    use super::{BinaryFetcher, DownloadError, FileOps, install_artwork, replace_pair_with_ops};

    struct FakeFetcher {
        calls: Cell<usize>,
        fail_on: Option<usize>,
    }

    impl BinaryFetcher for FakeFetcher {
        fn fetch_to(&self, url: &Url, destination: &mut File) -> Result<(), DownloadError> {
            let call = self.calls.get() + 1;
            self.calls.set(call);
            if self.fail_on == Some(call) {
                return Err(DownloadError::Fetch("simulated failure".into()));
            }
            destination.write_all(url.path().as_bytes())?;
            Ok(())
        }
    }

    struct FailingRenameOps {
        calls: Cell<usize>,
        fail_on: usize,
        fail_remove: bool,
    }

    impl FileOps for FailingRenameOps {
        fn exists(&self, path: &std::path::Path) -> std::io::Result<bool> {
            match fs::symlink_metadata(path) {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error),
            }
        }

        fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
            let call = self.calls.get() + 1;
            self.calls.set(call);
            if call == self.fail_on {
                return Err(std::io::Error::other("simulated replacement failure"));
            }
            fs::rename(from, to)
        }

        fn remove(&self, path: &std::path::Path) -> std::io::Result<()> {
            if self.fail_remove {
                return Err(std::io::Error::other("simulated cleanup failure"));
            }
            fs::remove_file(path)
        }
    }

    fn artwork() -> Artwork {
        Artwork {
            background_url: Url::parse("https://images.test/new-background").unwrap(),
            cover_url: Url::parse("https://images.test/new-cover").unwrap(),
        }
    }

    #[test]
    fn installs_artwork_in_fixed_directories() {
        let output = tempdir().unwrap();
        let fetcher = FakeFetcher {
            calls: Cell::new(0),
            fail_on: None,
        };

        let installed = install_artwork(&fetcher, &artwork(), output.path()).unwrap();

        assert_eq!(fs::read(&installed.background).unwrap(), b"/new-background");
        assert_eq!(fs::read(&installed.cover).unwrap(), b"/new-cover");
        assert!(installed.background.ends_with("background/background.jpg"));
        assert!(installed.cover.ends_with("cover/cover.jpg"));
        assert!(installed.background.is_absolute());
        assert!(installed.cover.is_absolute());
    }

    #[test]
    fn creates_missing_output_parent_directories() {
        let root = tempdir().unwrap();
        let output = root.path().join("missing/parents/artwork");
        assert!(!output.exists());
        let fetcher = FakeFetcher {
            calls: Cell::new(0),
            fail_on: None,
        };

        let installed = install_artwork(&fetcher, &artwork(), &output).unwrap();

        assert!(output.is_dir());
        assert!(installed.background.is_file());
        assert!(installed.cover.is_file());
    }

    #[test]
    fn second_download_failure_preserves_existing_files() {
        let output = tempdir().unwrap();
        let background = output.path().join("background/background.jpg");
        let cover = output.path().join("cover/cover.jpg");
        fs::create_dir_all(background.parent().unwrap()).unwrap();
        fs::create_dir_all(cover.parent().unwrap()).unwrap();
        fs::write(&background, b"old-background").unwrap();
        fs::write(&cover, b"old-cover").unwrap();
        let fetcher = FakeFetcher {
            calls: Cell::new(0),
            fail_on: Some(2),
        };

        assert!(install_artwork(&fetcher, &artwork(), output.path()).is_err());

        assert_eq!(fs::read(background).unwrap(), b"old-background");
        assert_eq!(fs::read(cover).unwrap(), b"old-cover");
    }

    #[test]
    fn successful_download_replaces_existing_files_without_temporary_artifacts() {
        let output = tempdir().unwrap();
        let background = output.path().join("background/background.jpg");
        let cover = output.path().join("cover/cover.jpg");
        fs::create_dir_all(background.parent().unwrap()).unwrap();
        fs::create_dir_all(cover.parent().unwrap()).unwrap();
        fs::write(&background, b"old-background").unwrap();
        fs::write(&cover, b"old-cover").unwrap();
        let fetcher = FakeFetcher {
            calls: Cell::new(0),
            fail_on: None,
        };

        install_artwork(&fetcher, &artwork(), output.path()).unwrap();

        assert_eq!(fs::read(&background).unwrap(), b"/new-background");
        assert_eq!(fs::read(&cover).unwrap(), b"/new-cover");
        assert_eq!(
            fs::read_dir(background.parent().unwrap()).unwrap().count(),
            1
        );
        assert_eq!(fs::read_dir(cover.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn second_replacement_failure_restores_both_old_files() {
        let output = tempdir().unwrap();
        let background_dir = output.path().join("background");
        let cover_dir = output.path().join("cover");
        fs::create_dir_all(&background_dir).unwrap();
        fs::create_dir_all(&cover_dir).unwrap();
        let background = background_dir.join("background.jpg");
        let cover = cover_dir.join("cover.jpg");
        let staged_background = background_dir.join("staged-background");
        let staged_cover = cover_dir.join("staged-cover");
        fs::write(&background, b"old-background").unwrap();
        fs::write(&cover, b"old-cover").unwrap();
        fs::write(&staged_background, b"new-background").unwrap();
        fs::write(&staged_cover, b"new-cover").unwrap();
        let ops = FailingRenameOps {
            calls: Cell::new(0),
            fail_on: 4,
            fail_remove: false,
        };

        assert!(
            replace_pair_with_ops(&ops, &staged_background, &background, &staged_cover, &cover,)
                .is_err()
        );

        assert_eq!(fs::read(&background).unwrap(), b"old-background");
        assert_eq!(fs::read(&cover).unwrap(), b"old-cover");
        assert_eq!(fs::read_dir(background_dir).unwrap().count(), 1);
        assert_eq!(fs::read_dir(cover_dir).unwrap().count(), 1);
    }

    #[test]
    fn cover_backup_and_cleanup_failure_still_restores_old_background() {
        let output = tempdir().unwrap();
        let background_dir = output.path().join("background");
        let cover_dir = output.path().join("cover");
        fs::create_dir_all(&background_dir).unwrap();
        fs::create_dir_all(&cover_dir).unwrap();
        let background = background_dir.join("background.jpg");
        let cover = cover_dir.join("cover.jpg");
        let staged_background = background_dir.join("staged-background");
        let staged_cover = cover_dir.join("staged-cover");
        fs::write(&background, b"old-background").unwrap();
        fs::write(&cover, b"old-cover").unwrap();
        fs::write(&staged_background, b"new-background").unwrap();
        fs::write(&staged_cover, b"new-cover").unwrap();
        let ops = FailingRenameOps {
            calls: Cell::new(0),
            fail_on: 2,
            fail_remove: true,
        };

        let error =
            replace_pair_with_ops(&ops, &staged_background, &background, &staged_cover, &cover)
                .unwrap_err()
                .to_string();

        assert_eq!(fs::read(&background).unwrap(), b"old-background");
        assert_eq!(fs::read(&cover).unwrap(), b"old-cover");
        assert!(error.contains("cleanup"));
    }
}
