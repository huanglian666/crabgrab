# CrabGrab MediaInfo Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `crabgrab mediainfo -i <FILE> -o <DIRECTORY>` that writes MediaInfoLib's English standard text report to `mediainfo.txt` without requiring users to install MediaInfo.

**Architecture:** Rust owns CLI dispatch, validation, report normalization, and transactional file replacement. A safe Rust wrapper calls a narrow C ABI implemented by a small C++ bridge; MediaInfoLib and ZenLib are pinned and statically linked into CrabGrab for Windows x86_64 and macOS arm64.

**Tech Stack:** Rust 2024, clap, thiserror, tempfile, CMake, C++17, MediaInfoLib, ZenLib, GitHub Actions.

## Global Constraints

- Work directly in `/Users/huanglian/Desktop/rust_code/crabgrab`; do not create a Git worktree.
- Keep the existing TMDB top-level `-i/--id -o/--output`, config, version, and help behavior unchanged.
- The MediaInfo command performs no network access and does not read TMDB configuration.
- Reports use MediaInfoLib's standard, non-`Complete`, English text view encoded as UTF-8.
- Normalize report line endings to LF and end the file with exactly one LF.
- Existing `mediainfo.txt` is replaced transactionally; failures preserve or restore the old file.
- Official release targets are `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`; Linux remains an uncommitted future target.
- End users must not need a MediaInfo executable or shared library.
- Use TDD for every Rust behavior and verify all existing tests remain green.

---

### Task 1: Rust report workflow and transactional installation

**Files:**
- Create: `src/media_info.rs`
- Create: `src/media_info/install.rs`
- Modify: `src/lib.rs`
- Test: unit tests inside `src/media_info.rs` and `src/media_info/install.rs`

**Interfaces:**
- Produces: `pub trait MediaAnalyzer { fn analyze(&self, input: &Path) -> Result<String, AnalyzeError>; }`
- Produces: `pub fn generate_report(analyzer: &impl MediaAnalyzer, input: &Path, output: &Path) -> Result<PathBuf, MediaInfoError>`
- Produces: `pub(crate) fn install_report(report: &str, output: &Path) -> Result<PathBuf, InstallError>`

- [ ] **Step 1: Write failing workflow tests**

Add tests with a `FakeAnalyzer` that returns a report or an error. Cover: missing input, directory input, output-directory creation, extensionless input acceptance, empty report rejection, General-only report rejection, CRLF normalization, exactly one trailing LF, old-report replacement, and preservation of the old report when analysis fails.

Core success test:

```rust
#[test]
fn writes_normalized_report_and_creates_output_directory() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("影片 sample");
    std::fs::write(&input, b"fixture").unwrap();
    let output = root.path().join("result");
    let analyzer = FakeAnalyzer::ok("General\r\nFormat : MPEG-4\r\n\r\nVideo\r\nFormat : AVC\r\n\r\n");

    let installed = generate_report(&analyzer, &input, &output).unwrap();

    assert_eq!(installed, output.join("mediainfo.txt"));
    assert_eq!(std::fs::read_to_string(installed).unwrap(), "General\nFormat : MPEG-4\n\nVideo\nFormat : AVC\n");
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `cargo test media_info --lib`

Expected: compilation fails because `media_info`, `MediaAnalyzer`, and `generate_report` do not exist.

- [ ] **Step 3: Implement minimal workflow types and validation**

Implement `AnalyzeError` and `MediaInfoError` with `thiserror`. `generate_report` must open the input for reading after confirming `metadata().is_file()`, create/validate the output directory, invoke the analyzer, reject blank reports and reports without a section header among `Video`, `Audio`, `Text`, `Other`, `Image`, or `Menu`, then call `install_report`.

Add `pub mod media_info;` to `src/lib.rs`.

- [ ] **Step 4: Implement transactional report installation**

Use `tempfile::NamedTempFile::new_in(output)` for the staged report and a unique `NamedTempFile`-derived backup path in the same directory. Write all bytes, call `flush()` and `as_file().sync_all()`, move an existing target to the unique backup path, persist the staged file to `mediainfo.txt`, remove the backup on success, and restore it if persistence fails. Error variants must retain the relevant path and both replacement/rollback errors when rollback fails.

- [ ] **Step 5: Run focused and full Rust tests**

Run: `cargo test media_info --lib`

Expected: all MediaInfo workflow tests pass.

Run: `cargo test`

Expected: all existing and new tests pass.

- [ ] **Step 6: Commit Task 1**

```bash
git add src/lib.rs src/media_info.rs src/media_info/install.rs Cargo.toml Cargo.lock
git commit -m "feat(mediainfo): add report installation workflow"
```

### Task 2: MediaInfo CLI subcommand and injectable dispatch

**Files:**
- Modify: `src/cli.rs`
- Modify: `tests/cli.rs`
- Create: `tests/mediainfo_cli.rs`

**Interfaces:**
- Consumes: `MediaAnalyzer` and `generate_report` from Task 1.
- Produces: `Command::MediaInfo { input: PathBuf, output: PathBuf }`
- Produces: an internal `run_with_services` dispatcher that accepts a MediaInfo analyzer for isolated CLI tests.

- [ ] **Step 1: Write failing clap and dispatch tests**

Verify both forms parse:

```rust
Cli::try_parse_from(["crabgrab", "mediainfo", "-i", "movie.mp4", "-o", "out"])
Cli::try_parse_from(["crabgrab", "mediainfo", "--input", "movie.mp4", "--output", "out"])
```

Add negative tests for either missing argument. Add an integration-style test with a fake analyzer proving that the subcommand writes `mediainfo.txt` without a config file or HTTP server. Preserve all existing CLI assertions.

- [ ] **Step 2: Run focused CLI tests and verify RED**

Run: `cargo test --test mediainfo_cli`

Expected: parsing fails because the `mediainfo` subcommand is absent.

- [ ] **Step 3: Add the subcommand and dispatch path**

Extend `Command`:

```rust
MediaInfo {
    #[arg(short = 'i', long, value_name = "FILE")]
    input: PathBuf,
    #[arg(short = 'o', long, value_name = "DIRECTORY")]
    output: PathBuf,
},
```

Add a `MediaInfo` variant to `AppError`. Refactor only enough dispatch code to inject a `MediaAnalyzer` in tests. Production dispatch uses `NativeMediaAnalyzer` from Task 3. The TMDB branch must retain its current validation/config/network ordering.

- [ ] **Step 4: Run CLI and regression tests**

Run: `cargo test --test mediainfo_cli --test cli --test tmdb_cli`

Expected: all listed tests pass.

- [ ] **Step 5: Commit Task 2**

```bash
git add src/cli.rs tests/cli.rs tests/mediainfo_cli.rs
git commit -m "feat(cli): add mediainfo subcommand"
```

### Task 3: Pin and build native MediaInfo dependencies

**Files:**
- Create: `.gitmodules`
- Create: `vendor/MediaInfoLib` as a pinned Git submodule
- Create: `vendor/ZenLib` as a pinned Git submodule
- Create: `native/CMakeLists.txt`
- Create: `build.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Produces: a static native target named `crabgrab_mediainfo_bridge` consumable by Cargo.
- Produces: build-time platform selection for Windows MSVC x86_64 and macOS arm64.

- [ ] **Step 1: Add pinned upstream sources**

Add official `MediaArea/MediaInfoLib` and `MediaArea/ZenLib` repositories as submodules under `vendor/`, checkout explicit commits from one compatible release, and record those commits in Git. Do not track moving branches.

- [ ] **Step 2: Write the native CMake configuration**

Configure C++17, disable shared libraries, and compile MediaInfoLib/ZenLib without GUI, curl/network, graph, or unrelated plugins. Define `crabgrab_mediainfo_bridge` and arrange for the final archive plus required C++ runtime libraries to be visible to Cargo.

- [ ] **Step 3: Wire Cargo to CMake**

Add the `cmake` crate under `[build-dependencies]`. In `build.rs`, emit `rerun-if-changed` directives for the bridge/CMake files and submodule commit directories, build the native target, emit static link search paths and libraries, and select the correct C++ runtime (`c++` on macOS; MSVC runtime is selected by the MSVC toolchain on Windows).

- [ ] **Step 4: Verify the native archive builds on macOS arm64**

Run: `cargo build -vv`

Expected: MediaInfoLib, ZenLib, and `crabgrab_mediainfo_bridge` compile and the Rust binary links without resolving a system MediaInfo library.

- [ ] **Step 5: Commit Task 3**

```bash
git add .gitmodules vendor native/CMakeLists.txt build.rs Cargo.toml Cargo.lock
git commit -m "build(mediainfo): statically link MediaInfoLib"
```

### Task 4: C++ bridge and safe Rust wrapper

**Files:**
- Create: `native/mediainfo_bridge.h`
- Create: `native/mediainfo_bridge.cpp`
- Create: `src/media_info/native.rs`
- Modify: `src/media_info.rs`
- Test: unit tests in `src/media_info/native.rs`

**Interfaces:**
- Consumes: static native build from Task 3.
- Produces: `pub struct NativeMediaAnalyzer;`
- Produces: `impl MediaAnalyzer for NativeMediaAnalyzer`.
- Produces C ABI: platform path entry points returning an owned `CrabGrabMediaInfoResult`, plus `crabgrab_mediainfo_result_free`.

- [ ] **Step 1: Define the ABI header and Rust mirror**

The result structure contains `status: i32`, report pointer/length, and error pointer/length. Provide a UTF-16 entry point on Windows and a byte-path entry point on Unix. Document that success has status zero and exactly one report buffer; failure has nonzero status and exactly one error buffer.

- [ ] **Step 2: Write failing wrapper protocol tests**

Factor result decoding into a private function whose tests use synthetic structures. Cover valid success, valid error, null result, pointer/length mismatch, both buffers set, neither buffer set, and invalid UTF-8. Confirm every owned synthetic result is released exactly once through an injectable releaser.

- [ ] **Step 3: Run wrapper tests and verify RED**

Run: `cargo test media_info::native --lib`

Expected: compilation fails because the decoder and native analyzer do not exist.

- [ ] **Step 4: Implement the C++ bridge**

For Windows, construct the MediaInfo path from the provided UTF-16 range. For macOS, construct it from the supplied native byte range. Set MediaInfo options for English output and non-`Complete` mode, open exactly one file, call `Inform()`, convert the result to UTF-8, and return an owned result. Catch `std::exception` and all unknown exceptions. A single free function destroys the complete result and its buffers using the same C++ runtime that allocated them.

- [ ] **Step 5: Implement the safe Rust analyzer**

Keep all FFI declarations and `unsafe` blocks in `src/media_info/native.rs`. Convert Windows `OsStr` with `encode_wide`; on Unix pass `OsStrExt::as_bytes`. Check lengths before conversion to C ABI sizes. Decode and validate the result before freeing it with an RAII guard. Map all bridge/protocol failures into `AnalyzeError`.

- [ ] **Step 6: Run wrapper tests and native smoke test**

Run: `cargo test media_info::native --lib`

Expected: protocol tests pass.

Run the analyzer against the fixed fixture added in Task 5 and assert its report contains `General` and `Video`.

- [ ] **Step 7: Commit Task 4**

```bash
git add native/mediainfo_bridge.h native/mediainfo_bridge.cpp native/CMakeLists.txt src/media_info.rs src/media_info/native.rs
git commit -m "feat(mediainfo): bridge MediaInfoLib into Rust"
```

### Task 5: Cross-platform fixtures, integration coverage, and licenses

**Files:**
- Create: `tests/fixtures/sample.mp4`
- Create: `tests/fixtures/README.md`
- Create: `tests/native_mediainfo.rs`
- Create: `THIRD_PARTY_LICENSES.md`

**Interfaces:**
- Consumes: `NativeMediaAnalyzer` from Task 4.
- Produces: deterministic integration evidence for standard English report generation and non-ASCII paths.

- [ ] **Step 1: Add a tiny licensed media fixture**

Use a very small MP4 whose origin and redistribution terms are recorded in `tests/fixtures/README.md`. Record its SHA-256 so fixture changes are intentional.

- [ ] **Step 2: Write native integration tests**

Test that the fixture report contains `General`, `Video`, `Format`, and `Complete name`; copy the fixture to a temporary path containing `中文 media` and analyze it again. Test an empty file and a plain-text file return controlled errors. Normalize only absolute paths and line endings before comparing stable report fragments.

- [ ] **Step 3: Run integration tests on macOS arm64**

Run: `cargo test --test native_mediainfo -- --nocapture`

Expected: all native tests pass without a system MediaInfo installation.

- [ ] **Step 4: Add required third-party notices**

Document the pinned MediaInfoLib and ZenLib versions, upstream URLs, licenses, and the MediaInfo binary redistribution attribution sentence required by the upstream license. Include notices for any optional third-party code actually compiled into the static library.

- [ ] **Step 5: Commit Task 5**

```bash
git add tests/fixtures tests/native_mediainfo.rs THIRD_PARTY_LICENSES.md
git commit -m "test(mediainfo): cover native reports and Unicode paths"
```

### Task 6: GitHub Actions release matrix and final verification

**Files:**
- Create or modify: `.github/workflows/ci.yml`
- Create or modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: complete Cargo/native build and fixture tests.
- Produces: verified Windows x86_64 and macOS arm64 release archives with checksums.

- [ ] **Step 1: Add CI matrix**

Use `windows-latest` with `x86_64-pc-windows-msvc` and an Apple Silicon macOS runner with `aarch64-apple-darwin`. Checkout recursively with submodules. Install only build-time CMake/C++ prerequisites; do not install MediaInfo. Run formatting once and run clippy, tests, release build, native-link inspection, and CLI fixture smoke tests for each target.

- [ ] **Step 2: Add release packaging**

On version tags, package `crabgrab.exe` or `crabgrab`, `LICENSE`, and `THIRD_PARTY_LICENSES.md` into platform-labelled archives. Generate SHA-256 files and upload archives/checksums to the GitHub release.

- [ ] **Step 3: Verify formatting, linting, tests, and release build locally**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo build --release
```

Expected: every command exits zero.

- [ ] **Step 4: Inspect the macOS release binary**

Run: `otool -L target/release/crabgrab`

Expected: no `libmediainfo.dylib` or `libzen.dylib` dependency appears.

Run the release CLI against `tests/fixtures/sample.mp4` in a temporary directory and verify `mediainfo.txt` contains `General` and `Video`.

- [ ] **Step 5: Review the final diff and commit**

Run: `git diff --check` and `git status --short`.

Commit only the CI/release files if all verification succeeds:

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml
git commit -m "ci: build embedded MediaInfo releases"
```

## Plan Self-Review

- Every design requirement maps to Tasks 1–6: CLI and isolation (Task 2), validation/report transaction (Task 1), pinned static native build (Task 3), ABI and exception/memory safety (Task 4), Unicode/native behavior and licenses (Task 5), and supported-platform delivery (Task 6).
- No Linux artifact, batch scan, custom report format, localization option, dynamic MediaInfo lookup, or unrelated TMDB refactor is included.
- Shared type names are consistent across tasks: `MediaAnalyzer`, `AnalyzeError`, `MediaInfoError`, `NativeMediaAnalyzer`, `generate_report`, and `install_report`.
