use std::fs::{self, FileType};
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use thiserror::Error;

const MAX_SIZE_ITERATIONS: usize = 32;

#[derive(Debug, Error)]
pub enum TreeReportError {
    #[error("cannot inspect tree path {path}: {source}")]
    Inspect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("tree video is not a regular file: {0}")]
    VideoNotFile(PathBuf),
    #[error("tree output is not a directory: {0}")]
    OutputNotDirectory(PathBuf),
    #[error("tree path has no UTF-8 file name: {0}")]
    InvalidName(PathBuf),
    #[error("external video conflicts with output entry: {0}")]
    NameConflict(PathBuf),
    #[error("tree report size did not converge")]
    SizeDidNotConverge,
    #[error("cannot install tree report {path}: {source}")]
    Install {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug)]
struct Entry {
    name: String,
    kind: EntryKind,
}

#[derive(Debug)]
enum EntryKind {
    Directory(Vec<Entry>),
    File(u64),
    Report,
}

pub fn generate_tree_report(video: &Path, output: &Path) -> Result<PathBuf, TreeReportError> {
    let video_metadata = metadata(video)?;
    if !video_metadata.is_file() {
        return Err(TreeReportError::VideoNotFile(video.to_path_buf()));
    }
    let output_metadata = metadata(output)?;
    if !output_metadata.is_dir() {
        return Err(TreeReportError::OutputNotDirectory(output.to_path_buf()));
    }

    let output_name = directory_name(output)?;
    let report_name = format!("{output_name}tree.txt");
    let report_path = output.join(&report_name);
    let mut entries = scan_directory(output, &report_path)?;
    entries.push(Entry {
        name: report_name,
        kind: EntryKind::Report,
    });

    let canonical_output = canonicalize(output)?;
    let canonical_video = canonicalize(video)?;
    if !canonical_video.starts_with(&canonical_output) {
        let video_name = utf8_name(video)?;
        if entries.iter().any(|entry| entry.name == video_name) {
            return Err(TreeReportError::NameConflict(output.join(video_name)));
        }
        entries.push(Entry {
            name: video_name,
            kind: EntryKind::File(video_metadata.len()),
        });
    }
    sort_entries(&mut entries);

    let contents = render_stable(&output_name, &entries)?;
    install_report(&report_path, contents.as_bytes())?;
    Ok(report_path)
}

fn metadata(path: &Path) -> Result<fs::Metadata, TreeReportError> {
    fs::metadata(path).map_err(|source| TreeReportError::Inspect {
        path: path.to_path_buf(),
        source,
    })
}

fn canonicalize(path: &Path) -> Result<PathBuf, TreeReportError> {
    fs::canonicalize(path).map_err(|source| TreeReportError::Inspect {
        path: path.to_path_buf(),
        source,
    })
}

fn utf8_name(path: &Path) -> Result<String, TreeReportError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| TreeReportError::InvalidName(path.to_path_buf()))
}

fn directory_name(path: &Path) -> Result<String, TreeReportError> {
    let canonical = canonicalize(path)?;
    utf8_name(&canonical)
}

fn scan_directory(directory: &Path, report_path: &Path) -> Result<Vec<Entry>, TreeReportError> {
    let iterator = fs::read_dir(directory).map_err(|source| TreeReportError::Inspect {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut entries = Vec::new();
    for result in iterator {
        let directory_entry = result.map_err(|source| TreeReportError::Inspect {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = directory_entry.path();
        if path == report_path {
            continue;
        }
        let file_type = directory_entry
            .file_type()
            .map_err(|source| TreeReportError::Inspect {
                path: path.clone(),
                source,
            })?;
        if let Some(entry) = snapshot_entry(&path, file_type, report_path)? {
            entries.push(entry);
        }
    }
    sort_entries(&mut entries);
    Ok(entries)
}

fn snapshot_entry(
    path: &Path,
    file_type: FileType,
    report_path: &Path,
) -> Result<Option<Entry>, TreeReportError> {
    if file_type.is_symlink() {
        return Ok(None);
    }
    let name = utf8_name(path)?;
    let kind = if file_type.is_dir() {
        EntryKind::Directory(scan_directory(path, report_path)?)
    } else if file_type.is_file() {
        EntryKind::File(metadata(path)?.len())
    } else {
        return Ok(None);
    };
    Ok(Some(Entry { name, kind }))
}

fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|left, right| {
        entry_rank(&left.kind)
            .cmp(&entry_rank(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn entry_rank(kind: &EntryKind) -> u8 {
    match kind {
        EntryKind::Directory(_) => 0,
        EntryKind::File(_) | EntryKind::Report => 1,
    }
}

fn render_stable(root_name: &str, entries: &[Entry]) -> Result<String, TreeReportError> {
    let mut report_size = 0;
    for _ in 0..MAX_SIZE_ITERATIONS {
        let contents = render(root_name, entries, report_size);
        let next_size = contents.len() as u64;
        if next_size == report_size {
            return Ok(contents);
        }
        report_size = next_size;
    }
    Err(TreeReportError::SizeDidNotConverge)
}

fn render(root_name: &str, entries: &[Entry], report_size: u64) -> String {
    let mut output = format!("{root_name}/\n");
    render_entries(entries, "", report_size, &mut output);
    output
}

fn render_entries(entries: &[Entry], prefix: &str, report_size: u64, output: &mut String) {
    for (index, entry) in entries.iter().enumerate() {
        let last = index + 1 == entries.len();
        output.push_str(prefix);
        output.push_str(if last { "└── " } else { "├── " });
        output.push_str(&entry.name);
        match &entry.kind {
            EntryKind::Directory(children) => {
                output.push_str("/\n");
                let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
                render_entries(children, &child_prefix, report_size, output);
            }
            EntryKind::File(size) => {
                output.push_str("  [");
                output.push_str(&format_size(*size));
                output.push_str("]\n");
            }
            EntryKind::Report => {
                output.push_str("  [");
                output.push_str(&format_size(report_size));
                output.push_str("]\n");
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 0_usize;
    let mut value = bytes as f64;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}

fn install_report(path: &Path, contents: &[u8]) -> Result<(), TreeReportError> {
    let directory = path.parent().expect("report path has output parent");
    let mut temporary =
        NamedTempFile::new_in(directory).map_err(|source| TreeReportError::Install {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| TreeReportError::Install {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| TreeReportError::Install {
            path: path.to_path_buf(),
            source: error.error,
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{directory_name, format_size, generate_tree_report};

    #[test]
    fn formats_binary_size_boundaries() {
        assert_eq!(format_size(3), "3 B");
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(format_size(1024_u64.pow(4)), "1.00 TiB");
    }

    #[test]
    fn renders_nested_entries_directories_first_and_external_video() {
        let root = tempdir().unwrap();
        let output = root.path().join("火遮眼 (2025)");
        let screenshots = output.join("截图");
        fs::create_dir_all(&screenshots).unwrap();
        fs::write(screenshots.join("01.jpg"), b"jpg").unwrap();
        fs::write(output.join("说明.txt"), vec![0; 1024]).unwrap();
        let video = root.path().join("火遮眼 (2025).mp4");
        fs::write(&video, b"video").unwrap();

        let report = generate_tree_report(&video, &output).unwrap();
        let contents = fs::read_to_string(report).unwrap();

        assert!(contents.starts_with("火遮眼 (2025)/\n├── 截图/\n│   └── 01.jpg  [3 B]\n"));
        assert!(contents.contains("说明.txt  [1.00 KiB]\n"));
        assert!(contents.contains("火遮眼 (2025).mp4  [5 B]\n"));
    }

    #[test]
    fn report_lists_its_actual_installed_size_and_can_be_replaced() {
        let root = tempdir().unwrap();
        let output = root.path().join("Movie (2025)");
        fs::create_dir(&output).unwrap();
        let video = root.path().join("Movie (2025).mp4");
        fs::write(&video, vec![0; 2048]).unwrap();

        generate_tree_report(&video, &output).unwrap();
        fs::write(output.join("added.txt"), b"new").unwrap();
        let report = generate_tree_report(&video, &output).unwrap();
        let contents = fs::read_to_string(&report).unwrap();
        let size = fs::metadata(&report).unwrap().len();

        assert!(contents.contains(&format!("Movie (2025) tree.txt  [{}]", format_size(size))));
        assert!(contents.contains("added.txt  [3 B]"));
    }

    #[test]
    fn video_already_inside_output_is_not_listed_twice() {
        let root = tempdir().unwrap();
        let output = root.path().join("Movie");
        fs::create_dir(&output).unwrap();
        let video = output.join("Movie.mp4");
        fs::write(&video, b"video").unwrap();

        let report = generate_tree_report(&video, &output).unwrap();
        let contents = fs::read_to_string(report).unwrap();

        assert_eq!(contents.matches("Movie.mp4").count(), 1);
    }

    #[test]
    fn conflicting_external_video_keeps_existing_report() {
        let root = tempdir().unwrap();
        let output = root.path().join("Movie");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("Movie.mp4"), b"local").unwrap();
        let old_report = output.join("Movie tree.txt");
        fs::write(&old_report, b"old report").unwrap();
        let external = root.path().join("external/Movie.mp4");
        fs::create_dir(external.parent().unwrap()).unwrap();
        fs::write(&external, b"external").unwrap();

        assert!(generate_tree_report(&external, &output).is_err());
        assert_eq!(fs::read(old_report).unwrap(), b"old report");
        assert_eq!(fs::read(external).unwrap(), b"external");
    }

    #[test]
    fn resolves_dot_directory_name() {
        assert_eq!(directory_name(Path::new(".")).unwrap(), "crabgrab");
    }
}
