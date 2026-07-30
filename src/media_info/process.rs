use std::path::{Path, PathBuf};
use std::process::Command;

use crate::media_info::tool::verify_tool;
use crate::media_info::{AnalyzeError, MediaAnalyzer, MediaProbe, MediaProber, parse_probe_json};

pub struct ProcessMediaAnalyzer {
    executable: PathBuf,
    expected_sha256: String,
}

impl ProcessMediaAnalyzer {
    pub fn new(executable: PathBuf, expected_sha256: impl Into<String>) -> Self {
        Self {
            executable,
            expected_sha256: expected_sha256.into(),
        }
    }

    pub fn bundled(crabgrab_executable: &Path) -> Self {
        let release_name = if cfg!(windows) {
            "mediainfo.exe"
        } else {
            "mediainfo"
        };
        let release_path = crabgrab_executable
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("tools")
            .join(release_name);

        #[cfg(debug_assertions)]
        let executable = if release_path.is_file() {
            release_path
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(".crabgrab-tools/mediainfo")
                .join(target_triple())
                .join(development_name())
        };
        #[cfg(not(debug_assertions))]
        let executable = release_path;

        Self::new(executable, expected_sha256())
    }
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn target_triple() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(debug_assertions, target_os = "windows"))]
fn target_triple() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn development_name() -> &'static str {
    "mediainfo"
}

#[cfg(all(debug_assertions, target_os = "windows"))]
fn development_name() -> &'static str {
    "MediaInfo.exe"
}

#[cfg(target_os = "macos")]
fn expected_sha256() -> &'static str {
    "d070140e4d60b3f49aae1cab752d77dc3611aac451b6109b9d2b1812b602b17e"
}

#[cfg(target_os = "windows")]
fn expected_sha256() -> &'static str {
    "30f2828a45a1895b033c3cd7784581033327e7b393033c55f4a03bb15cab0d89"
}

impl MediaAnalyzer for ProcessMediaAnalyzer {
    fn analyze(&self, input: &Path) -> Result<String, AnalyzeError> {
        verify_tool(&self.executable, &self.expected_sha256)
            .map_err(|error| AnalyzeError::Failed(error.to_string()))?;
        let output = Command::new(&self.executable)
            .arg(input)
            .env_remove("LANGUAGE")
            .env("LC_ALL", "en_US.UTF-8")
            .env("LANG", "en_US.UTF-8")
            .output()
            .map_err(|error| {
                AnalyzeError::Failed(format!("cannot start bundled MediaInfo: {error}"))
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.chars().take(4096).collect::<String>();
            return Err(AnalyzeError::Failed(format!(
                "bundled MediaInfo exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| AnalyzeError::Failed("bundled MediaInfo returned non-UTF-8 output".into()))
    }
}

impl MediaProber for ProcessMediaAnalyzer {
    fn probe(&self, input: &Path) -> Result<MediaProbe, AnalyzeError> {
        verify_tool(&self.executable, &self.expected_sha256)
            .map_err(|error| AnalyzeError::Failed(error.to_string()))?;
        let output = Command::new(&self.executable)
            .arg("--Output=JSON")
            .arg(input)
            .env_remove("LANGUAGE")
            .env("LC_ALL", "en_US.UTF-8")
            .env("LANG", "en_US.UTF-8")
            .output()
            .map_err(|error| {
                AnalyzeError::Failed(format!("cannot start bundled MediaInfo: {error}"))
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.chars().take(4096).collect::<String>();
            return Err(AnalyzeError::Failed(format!(
                "bundled MediaInfo JSON probe exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }
        let json = String::from_utf8(output.stdout).map_err(|_| {
            AnalyzeError::Failed("bundled MediaInfo returned non-UTF-8 JSON".into())
        })?;
        parse_probe_json(&json)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use crate::media_info::{MediaAnalyzer, MediaProber};

    use super::ProcessMediaAnalyzer;

    #[test]
    fn runs_sidecar_without_a_shell_and_captures_standard_report() {
        let root = tempdir().unwrap();
        let executable = root.path().join("fake-mediainfo");
        fs::write(
            &executable,
            "#!/bin/sh\nif [ -n \"$2\" ]; then echo 'unexpected extra argument' >&2; exit 9; fi\nprintf 'General\\nComplete name : %s\\nLocale : %s\\n\\nVideo\\nFormat : AVC\\n' \"$1\" \"$LC_ALL\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let hash = crate::media_info::tool::sha256(&executable).unwrap();
        let input = root.path().join("中文 ; $movie.mp4");
        fs::write(&input, b"fixture").unwrap();

        let analyzer = ProcessMediaAnalyzer::new(executable, hash);
        let report = analyzer.analyze(&input).unwrap();

        assert!(report.contains("中文 ; $movie.mp4"));
        assert!(report.contains("Locale : en_US.UTF-8"));
        assert!(report.contains("Video"));
    }

    #[test]
    fn reports_nonzero_exit_with_stderr() {
        let root = tempdir().unwrap();
        let executable = root.path().join("fake-mediainfo");
        fs::write(&executable, "#!/bin/sh\necho 'bad media' >&2\nexit 7\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let hash = crate::media_info::tool::sha256(&executable).unwrap();

        let error = ProcessMediaAnalyzer::new(executable, hash)
            .analyze(root.path())
            .unwrap_err();

        assert!(error.to_string().contains("7"));
        assert!(error.to_string().contains("bad media"));
    }

    #[test]
    fn probes_json_with_output_option_and_preserves_input_path() {
        let root = tempdir().unwrap();
        let executable = root.path().join("fake-mediainfo");
        fs::write(
            &executable,
            r#"#!/bin/sh
if [ "$1" != "--Output=JSON" ]; then echo 'missing json option' >&2; exit 8; fi
if [ ! -f "$2" ]; then echo 'input path was not preserved' >&2; exit 9; fi
printf '{"media":{"track":[{"@type":"General","Duration":"90.250"},{"@type":"Video"},{"@type":"Text","StreamKindPos":"1","Format":"ASS","Default":"Yes"}]}}'
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let hash = crate::media_info::tool::sha256(&executable).unwrap();
        let input = root.path().join("中文 ; $movie.mkv");
        fs::write(&input, b"fixture").unwrap();

        let probe = ProcessMediaAnalyzer::new(executable, hash)
            .probe(&input)
            .unwrap();

        assert_eq!(probe.duration_ms, 90_250);
        assert_eq!(probe.subtitles.len(), 1);
    }
}
