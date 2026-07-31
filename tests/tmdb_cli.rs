use std::fs;

use clap::Parser;
use crabgrab::cli::{Cli, run_with_api_base};
use httpmock::Method::GET;
use httpmock::MockServer;
use reqwest::Url;
use tempfile::tempdir;

#[test]
fn movie_download_writes_background_and_cover_end_to_end() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/3/configuration")
            .header("authorization", "Bearer integration-secret");
        then.status(200).json_body_obj(&serde_json::json!({
            "images": {
                "secure_base_url": format!("{}/t/p/", server.base_url()),
                "poster_sizes": ["original"],
                "backdrop_sizes": ["original"]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/3/movie/550")
            .query_param("language", "zh-CN");
        then.status(200).json_body_obj(&serde_json::json!({
            "backdrop_path": "/background.jpg",
            "poster_path": "/cover.jpg"
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/t/p/original/background.jpg");
        then.status(200).body("background-bytes");
    });
    server.mock(|when, then| {
        when.method(GET).path("/t/p/original/cover.jpg");
        then.status(200).body("cover-bytes");
    });

    let root = tempdir().unwrap();
    let executable = root.path().join("bin/crabgrab");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(
        executable.parent().unwrap().join("config.toml"),
        "[tmdb]\napi_token='integration-secret'\nlanguage='zh-CN'\n",
    )
    .unwrap();
    let output = root.path().join("artwork");
    let cli = Cli::try_parse_from([
        "crabgrab",
        "-i",
        "tmdb-movie-550",
        "-o",
        output.to_str().unwrap(),
    ])
    .unwrap();

    run_with_api_base(
        cli,
        executable,
        Some(Url::parse(&format!("{}/3/", server.base_url())).unwrap()),
    )
    .unwrap();

    assert_eq!(
        fs::read(output.join("background/background.jpg")).unwrap(),
        b"background-bytes"
    );
    assert_eq!(
        fs::read(output.join("cover/cover.jpg")).unwrap(),
        b"cover-bytes"
    );
}

#[test]
fn combined_poster_action_writes_background_and_cover() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/3/configuration")
            .header("authorization", "Bearer integration-secret");
        then.status(200).json_body_obj(&serde_json::json!({
            "images": {
                "secure_base_url": format!("{}/t/p/", server.base_url()),
                "poster_sizes": ["original"],
                "backdrop_sizes": ["original"]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/3/movie/550")
            .query_param("language", "zh-CN");
        then.status(200).json_body_obj(&serde_json::json!({
            "backdrop_path": "/background.jpg",
            "poster_path": "/cover.jpg"
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/t/p/original/background.jpg");
        then.status(200).body("background-bytes");
    });
    server.mock(|when, then| {
        when.method(GET).path("/t/p/original/cover.jpg");
        then.status(200).body("cover-bytes");
    });

    let root = tempdir().unwrap();
    let executable = root.path().join("bin/crabgrab");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(
        executable.parent().unwrap().join("config.toml"),
        "[tmdb]\napi_token='integration-secret'\nlanguage='zh-CN'\n",
    )
    .unwrap();
    let output = root.path().join("artwork");
    let cli = Cli::try_parse_from(["crabgrab", "-p", "tmdb-movie-550", output.to_str().unwrap()])
        .unwrap();

    run_with_api_base(
        cli,
        executable,
        Some(Url::parse(&format!("{}/3/", server.base_url())).unwrap()),
    )
    .unwrap();

    assert_eq!(
        fs::read(output.join("background/background.jpg")).unwrap(),
        b"background-bytes"
    );
    assert_eq!(
        fs::read(output.join("cover/cover.jpg")).unwrap(),
        b"cover-bytes"
    );
}
