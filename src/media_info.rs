mod install;
mod process;
mod tool;

pub use process::ProcessMediaAnalyzer;

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use thiserror::Error;

use install::install_report;

#[derive(Debug, Clone, Error)]
pub enum AnalyzeError {
    #[error("MediaInfo analysis failed: {0}")]
    Failed(String),
}

pub trait MediaAnalyzer {
    fn analyze(&self, input: &Path) -> Result<String, AnalyzeError>;
}

#[derive(Debug, Error)]
pub enum MediaInfoError {
    #[error("cannot inspect media input {path}: {source}")]
    Input {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("media input is not a regular file: {0}")]
    NotFile(PathBuf),
    #[error("cannot prepare output directory {path}: {source}")]
    Output {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Analyze(#[from] AnalyzeError),
    #[error("MediaInfo returned no recognizable media stream")]
    InvalidReport,
    #[error(transparent)]
    Install(#[from] install::InstallError),
}

pub fn generate_report(
    analyzer: &impl MediaAnalyzer,
    input: &Path,
    output: &Path,
) -> Result<PathBuf, MediaInfoError> {
    let metadata = fs::metadata(input).map_err(|source| MediaInfoError::Input {
        path: input.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(MediaInfoError::NotFile(input.to_path_buf()));
    }
    File::open(input).map_err(|source| MediaInfoError::Input {
        path: input.to_path_buf(),
        source,
    })?;

    if output.exists() {
        if !output.is_dir() {
            return Err(MediaInfoError::Output {
                path: output.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "path is not a directory",
                ),
            });
        }
    } else {
        fs::create_dir_all(output).map_err(|source| MediaInfoError::Output {
            path: output.to_path_buf(),
            source,
        })?;
    }

    let report = analyzer.analyze(input)?;
    let has_general = report.lines().any(|line| line.trim() == "General");
    let has_stream = report.lines().any(|line| {
        matches!(
            line.trim(),
            "Video" | "Audio" | "Text" | "Image" | "Menu" | "Other"
        )
    });
    if report.trim().is_empty() || !has_general || !has_stream {
        return Err(MediaInfoError::InvalidReport);
    }

    install_report(&report, output).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{AnalyzeError, MediaAnalyzer, generate_report};

    struct FakeAnalyzer(Result<String, AnalyzeError>);

    impl MediaAnalyzer for FakeAnalyzer {
        fn analyze(&self, _input: &Path) -> Result<String, AnalyzeError> {
            self.0.clone()
        }
    }

    fn valid_report() -> String {
        "General\r\nFormat : MPEG-4\r\n\r\nVideo\r\nFormat : AVC\r\n\r\n".into()
    }

    #[test]
    fn writes_normalized_report_and_creates_output_directory() {
        let root = tempdir().unwrap();
        let input = root.path().join("影片 sample");
        fs::write(&input, b"fixture").unwrap();
        let output = root.path().join("result");
        let analyzer = FakeAnalyzer(Ok(valid_report()));

        let installed = generate_report(&analyzer, &input, &output).unwrap();

        assert_eq!(installed, output.join("mediainfo.txt"));
        assert_eq!(
            fs::read_to_string(installed).unwrap(),
            "General\nFormat : MPEG-4\n\nVideo\nFormat : AVC\n"
        );
    }

    #[test]
    fn rejects_missing_input_without_touching_old_report() {
        let root = tempdir().unwrap();
        let output = root.path().join("result");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("mediainfo.txt"), "old\n").unwrap();

        let result = generate_report(
            &FakeAnalyzer(Ok(valid_report())),
            &root.path().join("missing.mp4"),
            &output,
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(output.join("mediainfo.txt")).unwrap(),
            "old\n"
        );
    }

    #[test]
    fn rejects_general_only_report_and_preserves_old_report() {
        let root = tempdir().unwrap();
        let input = root.path().join("movie");
        let output = root.path().join("result");
        fs::write(&input, b"fixture").unwrap();
        fs::create_dir(&output).unwrap();
        fs::write(output.join("mediainfo.txt"), "old\n").unwrap();

        let result = generate_report(
            &FakeAnalyzer(Ok("General\nFormat : Binary\n".into())),
            &input,
            &output,
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(output.join("mediainfo.txt")).unwrap(),
            "old\n"
        );
    }
}
