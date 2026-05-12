use std::collections::VecDeque;
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use codex_image::updater::{
    current_platform, parse_release_metadata, release_asset_name_for_tag,
    replacement_supported_for_os, resolve_artifact, run_update, run_update_with_installer,
    selected_archive_checksum, validate_archive_bytes, verify_archive_checksum, ArchiveKind,
    BinaryInstaller, GitHubClientTimeouts, GitHubReleaseClient, Platform, UpdateError,
    UpdateOptions, UpdateResult, UpdateSource, CHECKSUMS_ASSET_NAME,
};
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use tar::Builder;
use tempfile::tempdir;
use zip::write::FileOptions;

#[test]
fn release_workflow_targets_map_to_expected_artifact_names() {
    let cases = [
        (
            Platform::new("linux", "x86_64").expect("linux platform"),
            "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz",
        ),
        (
            Platform::new("macos", "x86_64").expect("macos x86_64 platform"),
            "codex-image-v1.2.3-x86_64-apple-darwin.tar.gz",
        ),
        (
            Platform::new("macos", "aarch64").expect("macos arm platform"),
            "codex-image-v1.2.3-aarch64-apple-darwin.tar.gz",
        ),
        (
            Platform::new("windows", "x86_64").expect("windows platform"),
            "codex-image-v1.2.3-x86_64-pc-windows-msvc.zip",
        ),
    ];

    for (platform, expected_asset_name) in cases {
        let actual = release_asset_name_for_tag("v1.2.3", &platform);
        assert_eq!(actual, expected_asset_name);
    }
}

#[test]
fn unsupported_os_arch_maps_to_typed_error() {
    let err = Platform::new("freebsd", "x86_64").expect_err("unsupported platform must fail");
    assert!(matches!(err, UpdateError::UnsupportedPlatform));
}

#[test]
fn resolves_exactly_one_asset_for_platform() {
    let platform = Platform::new("linux", "x86_64").expect("linux platform");
    let metadata = parse_release_metadata(release_fixture()).expect("fixture parses");

    let resolved = resolve_artifact(&metadata, &platform).expect("linux asset resolves");

    assert_eq!(resolved.version, "v1.2.3");
    assert_eq!(
        resolved.asset_name,
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz"
    );
    assert_eq!(resolved.archive_kind, ArchiveKind::TarGz);
    assert_eq!(resolved.checksum_asset_name, CHECKSUMS_ASSET_NAME);
    assert_eq!(
        resolved.checksum_download_url,
        "https://example.invalid/SHA256SUMS"
    );
}

#[test]
fn missing_checksum_asset_returns_typed_error() {
    let platform = Platform::new("linux", "x86_64").expect("linux platform");
    let metadata = parse_release_metadata(
        r#"{
            "tag_name": "v1.2.3",
            "assets": [
                {
                    "name": "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz",
                    "browser_download_url": "https://example.invalid/linux"
                }
            ]
        }"#,
    )
    .expect("fixture parses");

    let err = resolve_artifact(&metadata, &platform).expect_err("missing checksum must fail");
    assert!(matches!(err, UpdateError::MissingChecksumAsset));
}

#[test]
fn duplicate_checksum_asset_returns_typed_error() {
    let platform = Platform::new("linux", "x86_64").expect("linux platform");
    let metadata = parse_release_metadata(
        r#"{
            "tag_name": "v1.2.3",
            "assets": [
                {
                    "name": "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz",
                    "browser_download_url": "https://example.invalid/linux"
                },
                {
                    "name": "SHA256SUMS",
                    "browser_download_url": "https://example.invalid/checksums-a"
                },
                {
                    "name": "SHA256SUMS",
                    "browser_download_url": "https://example.invalid/checksums-b"
                }
            ]
        }"#,
    )
    .expect("fixture parses");

    let err = resolve_artifact(&metadata, &platform).expect_err("duplicate checksum must fail");
    assert!(matches!(err, UpdateError::DuplicateChecksumAsset));
}

#[test]
fn missing_target_asset_returns_typed_error() {
    let platform = Platform::new("macos", "aarch64").expect("macos arm platform");
    let metadata = parse_release_metadata(
        r#"{
            "tag_name": "v1.2.3",
            "assets": [
                {
                    "name": "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz",
                    "browser_download_url": "https://example.invalid/linux"
                }
            ]
        }"#,
    )
    .expect("fixture parses");

    let err = resolve_artifact(&metadata, &platform).expect_err("missing target must fail");
    assert!(matches!(err, UpdateError::MissingReleaseAsset));
}

#[test]
fn duplicate_target_asset_returns_typed_error() {
    let platform = Platform::new("linux", "x86_64").expect("linux platform");
    let metadata = parse_release_metadata(
        r#"{
            "tag_name": "v1.2.3",
            "assets": [
                {
                    "name": "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz",
                    "browser_download_url": "https://example.invalid/linux-a"
                },
                {
                    "name": "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz",
                    "browser_download_url": "https://example.invalid/linux-b"
                }
            ]
        }"#,
    )
    .expect("fixture parses");

    let err = resolve_artifact(&metadata, &platform).expect_err("duplicate target must fail");
    assert!(matches!(err, UpdateError::DuplicateReleaseAsset));
}

#[test]
fn parses_sha256sum_text_for_selected_archive() {
    let archive = b"archive bytes";
    let asset_name = "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz";
    let checksums = format!(
        "{}  other-asset.tar.gz\n{} *{asset_name}\n",
        sha256_hex(b"other bytes"),
        sha256_hex(archive)
    );

    let selected = selected_archive_checksum(&checksums, asset_name).expect("entry resolves");
    assert_eq!(selected, sha256_hex(archive));
    verify_archive_checksum(&checksums, asset_name, archive).expect("checksum matches");
}

#[test]
fn rejects_malformed_sha256sum_text() {
    let cases = [
        "",
        "not-a-digest  archive.tar.gz\n",
        "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz  archive.tar.gz\n",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  archive with spaces.tar.gz\n",
        "\n",
    ];

    for checksums in cases {
        let err = selected_archive_checksum(checksums, "archive.tar.gz")
            .expect_err("malformed checksum text must fail");
        assert!(matches!(err, UpdateError::ChecksumMetadataInvalid));
    }
}

#[test]
fn rejects_missing_duplicate_and_mismatched_selected_checksum_entries() {
    let asset_name = "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz";
    let archive = b"archive bytes";
    let digest = sha256_hex(archive);

    let missing =
        selected_archive_checksum(&format!("{}  other-asset.tar.gz\n", digest), asset_name)
            .expect_err("missing selected entry must fail");
    assert!(matches!(missing, UpdateError::ChecksumEntryMissing));

    let duplicate = selected_archive_checksum(
        &format!("{digest}  {asset_name}\n{digest} *{asset_name}\n"),
        asset_name,
    )
    .expect_err("duplicate selected entry must fail");
    assert!(matches!(duplicate, UpdateError::ChecksumEntryDuplicate));

    let mismatch = verify_archive_checksum(
        &format!("{}  {asset_name}\n", sha256_hex(b"tampered")),
        asset_name,
        archive,
    )
    .expect_err("mismatched selected entry must fail");
    assert!(matches!(mismatch, UpdateError::ChecksumMismatch));
}

#[test]
fn malformed_release_json_returns_typed_error() {
    let err = parse_release_metadata("{not valid json").expect_err("must fail");
    assert!(matches!(err, UpdateError::ReleaseMetadataInvalid));
}

#[test]
fn github_client_uses_default_timeouts_when_not_overridden() {
    let client = GitHubReleaseClient::with_api_base("acme/codex-image", "http://127.0.0.1")
        .expect("client should build");

    assert_eq!(client.timeouts(), GitHubClientTimeouts::default());
}

#[test]
fn github_release_lookup_timeout_maps_to_typed_error() {
    let (base_url, handle) = spawn_hanging_http_server();
    let client = GitHubReleaseClient::with_api_base_and_timeouts(
        "acme/codex-image",
        &base_url,
        GitHubClientTimeouts::new(Duration::from_millis(50), Duration::from_millis(50)),
    )
    .expect("client should build");

    let err = client
        .fetch_release(None)
        .expect_err("lookup should timeout");
    assert!(matches!(err, UpdateError::ReleaseLookupFailed));

    handle.join().expect("server thread should join");
}

#[test]
fn github_asset_download_timeout_maps_to_typed_error() {
    let (base_url, handle) = spawn_hanging_http_server();
    let client = GitHubReleaseClient::with_api_base_and_timeouts(
        "acme/codex-image",
        &base_url,
        GitHubClientTimeouts::new(Duration::from_millis(50), Duration::from_millis(50)),
    )
    .expect("client should build");

    let temp = tempdir().expect("tempdir");
    let destination = temp.path().join("asset.zip");
    let err = client
        .download_asset_to_path(&format!("{base_url}/asset.zip"), &destination)
        .expect_err("download should timeout");
    assert!(matches!(err, UpdateError::AssetDownloadFailed));

    handle.join().expect("server thread should join");
}

#[test]
fn replacement_policy_disables_confirmed_replacement_on_windows() {
    assert!(!replacement_supported_for_os("windows"));
    assert!(replacement_supported_for_os("linux"));
    assert!(replacement_supported_for_os("macos"));
}

#[test]
fn validates_tar_gz_archive_successfully() {
    let bytes = tar_gz_fixture(
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
        false,
        true,
        false,
    );

    let validated = validate_archive_bytes(
        ArchiveKind::TarGz,
        &bytes,
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
    )
    .expect("archive should validate");

    assert_eq!(
        validated.binary_path,
        Path::new("codex-image-v1.2.3-x86_64-unknown-linux-gnu/codex-image")
    );
}

#[test]
fn rejects_tar_gz_path_traversal() {
    let bytes = tar_gz_fixture_with_traversal();

    let err = validate_archive_bytes(
        ArchiveKind::TarGz,
        &bytes,
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
    )
    .expect_err("path traversal must fail");

    assert!(matches!(err, UpdateError::ArchivePathTraversal));
}

#[test]
fn rejects_wrong_top_level_directory() {
    let bytes = tar_gz_fixture("wrong-root", "codex-image", false, true, false);

    let err = validate_archive_bytes(
        ArchiveKind::TarGz,
        &bytes,
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
    )
    .expect_err("wrong root must fail");

    assert!(matches!(err, UpdateError::ArchiveTopLevelDirectoryMismatch));
}

#[test]
fn rejects_archive_missing_readme_files() {
    let bytes = tar_gz_fixture(
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
        true,
        false,
        false,
    );

    let err = validate_archive_bytes(
        ArchiveKind::TarGz,
        &bytes,
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
    )
    .expect_err("missing readme must fail");

    assert!(matches!(err, UpdateError::ArchiveMissingRequiredFile));
}

#[test]
fn rejects_archive_with_duplicate_binary() {
    let bytes = tar_gz_fixture(
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
        false,
        true,
        true,
    );

    let err = validate_archive_bytes(
        ArchiveKind::TarGz,
        &bytes,
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
    )
    .expect_err("duplicate binary must fail");

    assert!(matches!(err, UpdateError::ArchiveDuplicateBinary));
}

#[test]
fn validates_zip_archive_successfully() {
    let bytes = zip_fixture(
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
        false,
        true,
        false,
    );

    let validated = validate_archive_bytes(
        ArchiveKind::Zip,
        &bytes,
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
    )
    .expect("zip should validate");

    assert_eq!(
        validated.binary_path,
        Path::new("codex-image-v1.2.3-x86_64-pc-windows-msvc/codex-image.exe")
    );
    assert_eq!(
        validated.binary_archive_path,
        "codex-image-v1.2.3-x86_64-pc-windows-msvc/codex-image.exe"
    );
}

#[test]
fn rejects_zip_with_corrupt_payload_crc() {
    let bytes = zip_fixture_with_corrupt_payload_crc(
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
    );

    let err = validate_archive_bytes(
        ArchiveKind::Zip,
        &bytes,
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
    )
    .expect_err("corrupt payload crc must fail");

    assert!(matches!(err, UpdateError::ArchiveInvalid));
}

#[test]
fn zip_validation_records_forward_slash_archive_names() {
    let bytes = zip_fixture(
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
        false,
        true,
        false,
    );

    let validated = validate_archive_bytes(
        ArchiveKind::Zip,
        &bytes,
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
    )
    .expect("zip should validate");

    assert!(
        !validated.binary_archive_path.contains('\\'),
        "archive lookup path must remain slash-separated"
    );
    assert_eq!(
        validated.binary_archive_path,
        "codex-image-v1.2.3-x86_64-pc-windows-msvc/codex-image.exe"
    );
}

#[test]
fn rejects_tar_gz_with_corrupt_payload_crc() {
    let bytes = tar_gz_fixture_with_corrupt_payload_crc(
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
    );

    let err = validate_archive_bytes(
        ArchiveKind::TarGz,
        &bytes,
        "codex-image-v1.2.3-x86_64-unknown-linux-gnu",
        "codex-image",
    )
    .expect_err("corrupt tar/gzip payload must fail");

    assert!(matches!(err, UpdateError::ArchiveInvalid));
}

#[test]
fn rejects_zip_path_traversal() {
    let bytes = zip_fixture_with_traversal();

    let err = validate_archive_bytes(
        ArchiveKind::Zip,
        &bytes,
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
    )
    .expect_err("zip traversal must fail");

    assert!(matches!(err, UpdateError::ArchivePathTraversal));
}

#[test]
fn rejects_archive_missing_binary() {
    let bytes = zip_fixture(
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "other.exe",
        false,
        true,
        false,
    );

    let err = validate_archive_bytes(
        ArchiveKind::Zip,
        &bytes,
        "codex-image-v1.2.3-x86_64-pc-windows-msvc",
        "codex-image.exe",
    )
    .expect_err("missing binary must fail");

    assert!(matches!(err, UpdateError::ArchiveMissingRequiredFile));
}

#[test]
fn update_stops_on_release_lookup_error() {
    let source = FakeSource::new().with_release_result(Err(UpdateError::ReleaseLookupFailed));
    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let err = run_update(&source, &options(&binary_path, true)).expect_err("must fail");

    assert!(matches!(err, UpdateError::ReleaseLookupFailed));
    assert_eq!(source.download_calls(), 0);
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn update_stops_on_download_error() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_download_result(Err(UpdateError::AssetDownloadFailed));
    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let err = run_update(&source, &options(&binary_path, true)).expect_err("must fail");

    assert!(matches!(err, UpdateError::AssetDownloadFailed));
    assert_eq!(source.download_calls(), 1);
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn update_stops_on_checksum_download_error() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_download_result(Ok(download_archive_fixture()))
        .with_download_result(Err(UpdateError::AssetDownloadFailed));
    let installer = RecordingInstaller::default();
    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let err = run_update_with_installer(&source, &options(&binary_path, false), &installer)
        .expect_err("must fail");

    assert!(matches!(err, UpdateError::AssetDownloadFailed));
    assert_eq!(source.download_calls(), 2);
    assert_eq!(installer.calls(), 0);
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn update_stops_on_checksum_contract_errors_before_replacement() {
    let archive = download_archive_fixture();
    let asset_name = current_asset_name();
    let digest = sha256_hex(&archive);
    let cases = vec![
        ("empty", Vec::new(), UpdateError::ChecksumMetadataInvalid),
        (
            "malformed",
            b"not-a-checksum  archive.tar.gz\n".to_vec(),
            UpdateError::ChecksumMetadataInvalid,
        ),
        (
            "missing-selected-entry",
            format!("{digest}  other-asset.tar.gz\n").into_bytes(),
            UpdateError::ChecksumEntryMissing,
        ),
        (
            "duplicate-selected-entry",
            format!("{digest}  {asset_name}\n{digest} *{asset_name}\n").into_bytes(),
            UpdateError::ChecksumEntryDuplicate,
        ),
        (
            "mismatch",
            format!("{}  {asset_name}\n", sha256_hex(b"tampered archive")).into_bytes(),
            UpdateError::ChecksumMismatch,
        ),
    ];

    for (label, checksums, expected) in cases {
        let source = FakeSource::new()
            .with_release_result(Ok(
                parse_release_metadata(release_fixture()).expect("release fixture")
            ))
            .with_download_result(Ok(archive.clone()))
            .with_download_result(Ok(checksums));
        let installer = RecordingInstaller::default();
        let temp = tempdir().expect("tempdir");
        let binary_path = temp.path().join(current_binary_name());
        std::fs::write(&binary_path, b"old").expect("seed binary");

        let err =
            match run_update_with_installer(&source, &options(&binary_path, false), &installer) {
                Ok(_) => panic!("case {label} must fail"),
                Err(err) => err,
            };

        assert_eq!(err, expected, "case {label}");
        assert_eq!(source.download_calls(), 2, "case {label}");
        assert_eq!(installer.calls(), 0, "case {label}");
        assert_eq!(
            std::fs::read(&binary_path).expect("read binary"),
            b"old",
            "case {label}"
        );
    }
}

#[test]
fn checksum_mismatch_blocks_archive_validation_errors() {
    let invalid_archive = vec![1, 2, 3, 4];
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_download_result(Ok(invalid_archive))
        .with_download_result(Ok(format!(
            "{}  {}\n",
            sha256_hex(b"different bytes"),
            current_asset_name()
        )
        .into_bytes()));
    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let err = run_update(&source, &options(&binary_path, true)).expect_err("must fail");

    assert!(matches!(err, UpdateError::ChecksumMismatch));
    assert_eq!(source.download_calls(), 2);
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn update_stops_on_invalid_downloaded_archive() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_archive_and_valid_checksum(vec![1, 2, 3, 4]);
    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let err = run_update(&source, &options(&binary_path, true)).expect_err("must fail");

    assert!(
        matches!(
            err,
            UpdateError::ArchiveInvalid
                | UpdateError::ArchiveMissingRequiredFile
                | UpdateError::ArchiveTopLevelDirectoryMismatch
                | UpdateError::ArchivePathTraversal
                | UpdateError::ArchiveDuplicateBinary
        ),
        "expected archive validation error, got {err:?}"
    );
    assert_eq!(source.download_calls(), 2);
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn dry_run_downloads_and_validates_without_replacement() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let result = run_update(&source, &options(&binary_path, true)).expect("dry-run succeeds");

    assert_eq!(result, expected_result(&binary_path, "validated"));
    assert_eq!(source.download_calls(), 2);
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn dry_run_rejects_corrupt_payload_before_reporting_validated() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_archive_and_valid_checksum(corrupt_download_archive_fixture());

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let err = run_update(&source, &options(&binary_path, true)).expect_err("dry-run must fail");

    assert!(matches!(err, UpdateError::ArchiveInvalid));
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn dry_run_without_confirmation_is_allowed_and_still_fetches_release() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old").expect("seed binary");

    let mut update_options = options(&binary_path, true);
    update_options.confirm = false;

    let result = run_update(&source, &update_options).expect("dry-run should not require --yes");

    assert_eq!(result, expected_result(&binary_path, "validated"));
    assert_eq!(source.download_calls(), 2);
    assert_eq!(std::fs::read(&binary_path).expect("read binary"), b"old");
}

#[test]
fn successful_update_replaces_binary_contents() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old-binary").expect("seed binary");

    let result = run_update(&source, &options(&binary_path, false)).expect("update succeeds");

    assert_eq!(result, expected_result(&binary_path, "updated"));
    assert_eq!(source.download_calls(), 2);
    assert_eq!(
        std::fs::read(&binary_path).expect("read binary"),
        expected_updated_binary_bytes()
    );
}

#[test]
fn replacement_failure_restores_original_binary() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old-binary").expect("seed binary");

    let err = run_update_with_installer(&source, &options(&binary_path, false), &FailingInstaller)
        .expect_err("must fail");

    assert!(matches!(err, UpdateError::ReplacementFailed));
    assert_eq!(
        std::fs::read(&binary_path).expect("read binary"),
        b"old-binary"
    );
}

#[cfg(unix)]
#[test]
fn update_preserves_existing_unix_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old-binary").expect("seed binary");
    std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o751))
        .expect("set permissions");

    run_update(&source, &options(&binary_path, false)).expect("update succeeds");

    let mode = std::fs::metadata(&binary_path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o751);
}

fn expected_result(binary_path: &Path, status: &str) -> UpdateResult {
    let platform = current_platform().expect("supported platform for test host");
    UpdateResult {
        status: status.to_string(),
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        target_version: "v1.2.3".to_string(),
        target: platform.rust_target().to_string(),
        asset: release_asset_name_for_tag("v1.2.3", &platform),
        binary_path: binary_path.display().to_string(),
    }
}

fn expected_updated_binary_bytes() -> &'static [u8] {
    let platform = current_platform().expect("supported platform for test host");
    match platform.archive_kind() {
        ArchiveKind::TarGz => b"#!/bin/sh\necho codex-image\n",
        ArchiveKind::Zip => b"MZ",
    }
}

fn current_binary_name() -> &'static str {
    current_platform()
        .expect("supported platform for test host")
        .binary_name()
}

fn current_archive_root_and_binary() -> (String, String, ArchiveKind) {
    let platform = current_platform().expect("supported platform for test host");
    let asset = release_asset_name_for_tag("v1.2.3", &platform);
    let root = match platform.archive_kind() {
        ArchiveKind::TarGz => asset
            .strip_suffix(".tar.gz")
            .expect("tar.gz suffix")
            .to_string(),
        ArchiveKind::Zip => asset.strip_suffix(".zip").expect("zip suffix").to_string(),
    };

    (
        root,
        platform.binary_name().to_string(),
        platform.archive_kind(),
    )
}

fn download_archive_fixture() -> Vec<u8> {
    let (root, binary_name, archive_kind) = current_archive_root_and_binary();
    match archive_kind {
        ArchiveKind::TarGz => tar_gz_fixture(&root, &binary_name, false, true, false),
        ArchiveKind::Zip => zip_fixture(&root, &binary_name, false, true, false),
    }
}

fn checksums_fixture_for_archive(archive: &[u8]) -> Vec<u8> {
    let asset_name = current_asset_name();
    format!("{}  {asset_name}\n", sha256_hex(archive)).into_bytes()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn current_asset_name() -> String {
    let platform = current_platform().expect("supported platform for test host");
    release_asset_name_for_tag("v1.2.3", &platform)
}

fn corrupt_download_archive_fixture() -> Vec<u8> {
    let (root, binary_name, archive_kind) = current_archive_root_and_binary();
    match archive_kind {
        ArchiveKind::TarGz => tar_gz_fixture_with_corrupt_payload_crc(&root, &binary_name),
        ArchiveKind::Zip => zip_fixture_with_corrupt_payload_crc(&root, &binary_name),
    }
}

fn options(binary_path: &Path, dry_run: bool) -> UpdateOptions {
    UpdateOptions {
        current_executable: binary_path.to_path_buf(),
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        requested_version: None,
        dry_run,
        confirm: true,
    }
}

struct FailingInstaller;

impl BinaryInstaller for FailingInstaller {
    fn replace_binary(
        &self,
        _current_executable: &Path,
        _new_binary_bytes: &[u8],
    ) -> Result<(), UpdateError> {
        Err(UpdateError::ReplacementFailed)
    }
}

#[derive(Default)]
struct RecordingInstaller {
    calls: Mutex<usize>,
}

impl RecordingInstaller {
    fn calls(&self) -> usize {
        *self.calls.lock().expect("lock")
    }
}

impl BinaryInstaller for RecordingInstaller {
    fn replace_binary(
        &self,
        _current_executable: &Path,
        _new_binary_bytes: &[u8],
    ) -> Result<(), UpdateError> {
        *self.calls.lock().expect("lock") += 1;
        Ok(())
    }
}

#[derive(Default)]
struct FakeSource {
    release_results: Mutex<VecDeque<Result<codex_image::updater::ReleaseMetadata, UpdateError>>>,
    download_results: Mutex<VecDeque<Result<Vec<u8>, UpdateError>>>,
    download_calls: Mutex<usize>,
}

impl FakeSource {
    fn new() -> Self {
        Self::default()
    }

    fn with_release_result(
        self,
        result: Result<codex_image::updater::ReleaseMetadata, UpdateError>,
    ) -> Self {
        self.release_results.lock().expect("lock").push_back(result);
        self
    }

    fn with_download_result(self, result: Result<Vec<u8>, UpdateError>) -> Self {
        self.download_results
            .lock()
            .expect("lock")
            .push_back(result);
        self
    }

    fn with_valid_archive_downloads(self) -> Self {
        self.with_archive_and_valid_checksum(download_archive_fixture())
    }

    fn with_archive_and_valid_checksum(self, archive: Vec<u8>) -> Self {
        let checksums = checksums_fixture_for_archive(&archive);
        self.with_download_result(Ok(archive))
            .with_download_result(Ok(checksums))
    }

    fn download_calls(&self) -> usize {
        *self.download_calls.lock().expect("lock")
    }
}

impl UpdateSource for FakeSource {
    fn fetch_release(
        &self,
        requested_version: Option<&str>,
    ) -> Result<codex_image::updater::ReleaseMetadata, UpdateError> {
        if let Some(version) = requested_version {
            assert_eq!(version, "v1.2.3");
        }

        self.release_results
            .lock()
            .expect("lock")
            .pop_front()
            .expect("expected release result")
    }

    fn download_asset_to_path(
        &self,
        _download_url: &str,
        destination: &Path,
    ) -> Result<(), UpdateError> {
        *self.download_calls.lock().expect("lock") += 1;
        let next = self
            .download_results
            .lock()
            .expect("lock")
            .pop_front()
            .expect("expected download result");

        match next {
            Ok(bytes) => {
                std::fs::write(destination, bytes).map_err(|_| UpdateError::AssetDownloadFailed)
            }
            Err(err) => Err(err),
        }
    }
}

fn spawn_hanging_http_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind hanging server");
    let addr = listener.local_addr().expect("local addr");

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept one connection");
        let mut request_buffer = [0_u8; 1024];
        let _ = stream.read(&mut request_buffer);
        thread::sleep(Duration::from_millis(200));
    });

    (format!("http://{addr}"), handle)
}

fn release_fixture() -> &'static str {
    r#"{
        "tag_name": "v1.2.3",
        "assets": [
            {
                "name": "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz",
                "browser_download_url": "https://example.invalid/linux"
            },
            {
                "name": "codex-image-v1.2.3-x86_64-apple-darwin.tar.gz",
                "browser_download_url": "https://example.invalid/macos-intel"
            },
            {
                "name": "codex-image-v1.2.3-aarch64-apple-darwin.tar.gz",
                "browser_download_url": "https://example.invalid/macos-arm"
            },
            {
                "name": "codex-image-v1.2.3-x86_64-pc-windows-msvc.zip",
                "browser_download_url": "https://example.invalid/windows"
            },
            {
                "name": "SHA256SUMS",
                "browser_download_url": "https://example.invalid/SHA256SUMS"
            }
        ]
    }"#
}

fn tar_gz_fixture(
    top_level_dir: &str,
    binary_name: &str,
    omit_readme: bool,
    include_korean_readme: bool,
    duplicate_binary: bool,
) -> Vec<u8> {
    let mut compressed = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut tar = Builder::new(&mut compressed);

        append_tar_file(
            &mut tar,
            &format!("{top_level_dir}/{binary_name}"),
            b"#!/bin/sh\necho codex-image\n",
            0o755,
        );

        if duplicate_binary {
            append_tar_file(
                &mut tar,
                &format!("{top_level_dir}/nested/{binary_name}"),
                b"#!/bin/sh\n",
                0o755,
            );
        }

        if !omit_readme {
            append_tar_file(
                &mut tar,
                &format!("{top_level_dir}/README.md"),
                b"README",
                0o644,
            );
        }

        if include_korean_readme {
            append_tar_file(
                &mut tar,
                &format!("{top_level_dir}/README.ko.md"),
                b"README KO",
                0o644,
            );
        }

        tar.finish().expect("finish tar");
    }

    compressed.finish().expect("finish gzip")
}

fn tar_gz_fixture_with_traversal() -> Vec<u8> {
    let mut compressed = GzEncoder::new(Vec::new(), Compression::default());
    compressed
        .write_all(&raw_tar_with_single_file("../escape", b"nope", 0o644))
        .expect("write raw tar to gzip stream");
    compressed.finish().expect("finish gzip")
}

fn tar_gz_fixture_with_corrupt_payload_crc(top_level_dir: &str, binary_name: &str) -> Vec<u8> {
    let mut bytes = tar_gz_fixture(top_level_dir, binary_name, false, true, false);
    let truncate_to = bytes
        .len()
        .checked_sub(32)
        .expect("gzip fixture should have enough bytes to truncate");
    bytes.truncate(truncate_to);
    bytes
}

fn raw_tar_with_single_file(path: &str, content: &[u8], mode: u32) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    let header = build_tar_header(path, content.len() as u64, mode);
    tar_bytes.extend_from_slice(&header);
    tar_bytes.extend_from_slice(content);

    let file_padding = (512 - (content.len() % 512)) % 512;
    tar_bytes.extend(std::iter::repeat_n(0, file_padding));

    // End-of-archive marker: two all-zero blocks.
    tar_bytes.extend(std::iter::repeat_n(0, 1024));
    tar_bytes
}

fn build_tar_header(path: &str, size: u64, mode: u32) -> [u8; 512] {
    let mut header = [0_u8; 512];

    write_tar_bytes(&mut header, 0, 100, path.as_bytes());
    write_tar_octal(&mut header, 100, 8, mode as u64);
    write_tar_octal(&mut header, 108, 8, 0);
    write_tar_octal(&mut header, 116, 8, 0);
    write_tar_octal(&mut header, 124, 12, size);
    write_tar_octal(&mut header, 136, 12, 0);
    for byte in &mut header[148..156] {
        *byte = b' ';
    }
    header[156] = b'0';
    write_tar_bytes(&mut header, 257, 6, b"ustar");
    header[262] = 0;
    write_tar_bytes(&mut header, 263, 2, b"00");

    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let checksum_text = format!("{:06o}\0 ", checksum);
    write_tar_bytes(&mut header, 148, 8, checksum_text.as_bytes());

    header
}

fn write_tar_bytes(header: &mut [u8; 512], start: usize, width: usize, value: &[u8]) {
    let count = value.len().min(width);
    header[start..start + count].copy_from_slice(&value[..count]);
}

fn write_tar_octal(header: &mut [u8; 512], start: usize, width: usize, value: u64) {
    let mut text = format!("{:o}", value);
    if text.len() + 1 > width {
        text = "0".repeat(width.saturating_sub(1));
    }

    let padding = width - text.len() - 1;
    for idx in 0..padding {
        header[start + idx] = b'0';
    }
    let text_start = start + padding;
    header[text_start..text_start + text.len()].copy_from_slice(text.as_bytes());
    header[start + width - 1] = 0;
}

fn append_tar_file(
    tar: &mut Builder<&mut GzEncoder<Vec<u8>>>,
    path: &str,
    content: &[u8],
    mode: u32,
) {
    let mut header = tar::Header::new_gnu();
    header.set_mode(mode);
    header.set_size(content.len() as u64);
    header.set_cksum();
    tar.append_data(&mut header, path, Cursor::new(content))
        .expect("append tar entry");
}

fn zip_fixture(
    top_level_dir: &str,
    binary_name: &str,
    omit_readme: bool,
    include_korean_readme: bool,
    duplicate_binary: bool,
) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = FileOptions::default();

        zip.start_file(format!("{top_level_dir}/{binary_name}"), options)
            .expect("start binary");
        zip.write_all(b"MZ").expect("write binary");

        if duplicate_binary {
            zip.start_file(format!("{top_level_dir}/nested/{binary_name}"), options)
                .expect("start duplicate binary");
            zip.write_all(b"MZ").expect("write duplicate binary");
        }

        if !omit_readme {
            zip.start_file(format!("{top_level_dir}/README.md"), options)
                .expect("start readme");
            zip.write_all(b"README").expect("write readme");
        }

        if include_korean_readme {
            zip.start_file(format!("{top_level_dir}/README.ko.md"), options)
                .expect("start ko readme");
            zip.write_all(b"README KO").expect("write ko readme");
        }

        zip.finish().expect("finish zip");
    }

    cursor.into_inner()
}

fn zip_fixture_with_traversal() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = FileOptions::default();
        zip.start_file("../escape", options)
            .expect("start traversal entry");
        zip.write_all(b"nope").expect("write traversal entry");
        zip.finish().expect("finish zip");
    }
    cursor.into_inner()
}

fn zip_fixture_with_corrupt_payload_crc(top_level_dir: &str, binary_name: &str) -> Vec<u8> {
    let mut bytes = zip_fixture(top_level_dir, binary_name, false, true, false);
    let signature = b"PK\x01\x02";
    let Some(offset) = bytes
        .windows(signature.len())
        .position(|window| window == signature)
    else {
        panic!("central directory signature missing");
    };

    // Central directory layout: signature (4) + 12 bytes metadata + CRC32 (4).
    let crc_offset = offset + 16;
    bytes[crc_offset] ^= 0xFF;
    bytes
}
