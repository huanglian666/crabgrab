use std::process::{Command, Output};

fn crabgrab(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crabgrab"))
        .args(args)
        .output()
        .expect("crabgrab binary should run")
}

fn visible_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn short_version_flag_prints_package_version() {
    let output = crabgrab(&["-v"]);
    let expected = format!("crabgrab {}", env!("CARGO_PKG_VERSION"));

    assert!(output.status.success());
    assert!(visible_output(&output).contains(&expected));
}

#[test]
fn long_version_flag_matches_short_version_output() {
    let short = crabgrab(&["-v"]);
    let long = crabgrab(&["--version"]);
    let expected = format!("crabgrab {}", env!("CARGO_PKG_VERSION"));

    assert!(long.status.success());
    assert!(visible_output(&long).contains(&expected));
    assert_eq!(visible_output(&long), visible_output(&short));
}

#[test]
fn no_arguments_fails_and_displays_help() {
    let output = crabgrab(&[]);
    let visible = visible_output(&output);

    assert!(!output.status.success());
    assert!(visible.contains("crabgrab"));
    assert!(visible.contains("Usage"));
    assert!(visible.contains("-v"));
    assert!(visible.contains("--version"));
    assert!(visible.contains("-h"));
    assert!(visible.contains("--help"));
}

#[test]
fn help_flag_succeeds_and_displays_help() {
    let output = crabgrab(&["--help"]);
    let visible = visible_output(&output);

    assert!(output.status.success());
    assert!(visible.contains("crabgrab"));
    assert!(visible.contains("Usage"));
    assert!(visible.contains("-v"));
    assert!(visible.contains("--version"));
    assert!(visible.contains("-h"));
    assert!(visible.contains("--help"));
    assert!(visible.contains("-i"));
    assert!(visible.contains("--id"));
    assert!(visible.contains("-o"));
    assert!(visible.contains("--output"));
    assert!(visible.contains("-t"));
    assert!(visible.contains("--tree"));
    assert!(visible.contains("config"));
    assert!(visible.contains("sc"));
}

#[test]
fn download_arguments_must_be_provided_together() {
    let missing_output = crabgrab(&["-i", "tmdb-movie-550"]);
    let missing_id = crabgrab(&["-o", "/tmp/crabgrab-test-output"]);

    assert!(!missing_output.status.success());
    assert!(visible_output(&missing_output).contains("--output"));
    assert!(!missing_id.status.success());
    assert!(visible_output(&missing_id).contains("--id"));
}

#[test]
fn invalid_resource_id_fails_before_configuration_lookup() {
    let output = crabgrab(&["-i", "invalid", "-o", "/tmp/crabgrab-test-output"]);
    let visible = visible_output(&output);

    assert!(!output.status.success());
    assert!(visible.contains("tmdb-movie-550"));
    assert!(!visible.contains("config init"));
}

#[test]
fn download_arguments_conflict_with_config_subcommand() {
    let output = crabgrab(&[
        "-i",
        "tmdb-movie-550",
        "-o",
        "/tmp/crabgrab-test-output",
        "config",
        "init",
    ]);

    assert!(!output.status.success());
}
