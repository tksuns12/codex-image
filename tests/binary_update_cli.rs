use std::collections::VecDeque;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Mutex;

use assert_cmd::Command;
use codex_image::cli::execute_update_command;
use codex_image::diagnostics::CliError;
use codex_image::updater::{
    current_platform, parse_release_metadata, release_asset_name_for_tag, ArchiveKind,
    BinaryInstaller, ReleaseMetadata, UpdateError, UpdateSource,
};
use flate2::write::GzEncoder;
use flate2::Compression;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use tar::Builder;
use tempfile::tempdir;
use zip::write::FileOptions;

#[test]
fn update_cli_helper_no_flags_updates_latest_release_by_default() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();
    let installer = RecordingInstaller::default();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old-binary").expect("seed binary");

    let result = execute_update_command(
        &source,
        &installer,
        binary_path,
        env!("CARGO_PKG_VERSION").to_string(),
        false,
        false,
        None,
    )
    .expect("no-flag update should install latest release");

    assert_eq!(result.status, "updated");
    assert_eq!(result.target_version, "v1.2.3");
    assert_eq!(source.last_requested_version(), None);
    assert_eq!(source.download_calls(), 2);
    assert_eq!(installer.calls(), 1, "no-flag update must replace binary");
    assert_eq!(
        installer
            .last_bytes()
            .expect("installer should capture replacement payload"),
        expected_updated_binary_bytes().to_vec()
    );
}

#[test]
fn update_cli_invalid_version_with_output_json_uses_clap_stderr_not_json_envelope() {
    let mut cmd = Command::cargo_bin("codex-image").expect("binary exists");
    cmd.arg("update")
        .arg("--dry-run")
        .arg("--output")
        .arg("json")
        .arg("--version")
        .arg("1.2.3");

    cmd.assert()
        .code(2)
        .stderr(predicate::str::contains("version tag must start with 'v'"))
        .stderr(predicate::str::contains("\"error\":").not());
}

#[test]
fn update_cli_helper_dry_run_validates_without_replacement() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();
    let installer = RecordingInstaller::default();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old-binary").expect("seed binary");

    let result = execute_update_command(
        &source,
        &installer,
        binary_path.clone(),
        env!("CARGO_PKG_VERSION").to_string(),
        false,
        true,
        Some("v1.2.3".to_string()),
    )
    .expect("dry-run should succeed");

    assert_eq!(result.status, "validated");
    assert_eq!(result.target_version, "v1.2.3");
    assert_eq!(result.binary_path, binary_path.display().to_string());
    assert_eq!(source.last_requested_version(), Some("v1.2.3".to_string()));
    assert_eq!(source.download_calls(), 2);
    assert_eq!(installer.calls(), 0, "dry-run must not replace binary");
    assert_eq!(
        std::fs::read(&binary_path).expect("read binary"),
        b"old-binary"
    );
}

#[test]
fn update_cli_helper_confirmed_update_calls_replacement() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_valid_archive_downloads();
    let installer = RecordingInstaller::default();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());
    std::fs::write(&binary_path, b"old-binary").expect("seed binary");

    let result = execute_update_command(
        &source,
        &installer,
        binary_path,
        env!("CARGO_PKG_VERSION").to_string(),
        true,
        false,
        None,
    )
    .expect("confirmed update should succeed");

    assert_eq!(result.status, "updated");
    assert_eq!(source.download_calls(), 2);
    assert_eq!(installer.calls(), 1, "confirmed update must replace binary");
    assert_eq!(
        installer
            .last_bytes()
            .expect("installer should capture replacement payload"),
        expected_updated_binary_bytes().to_vec()
    );
}

#[test]
fn update_cli_helper_invalid_archive_maps_to_response_contract_cli_error() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_archive_and_valid_checksum(vec![1, 2, 3, 4]);
    let installer = RecordingInstaller::default();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());

    let err = execute_update_command(
        &source,
        &installer,
        binary_path,
        env!("CARGO_PKG_VERSION").to_string(),
        false,
        true,
        None,
    )
    .expect_err("invalid archive must fail");

    assert!(
        matches!(
            err,
            CliError::BinaryUpdate(
                UpdateError::ArchiveInvalid
                    | UpdateError::ArchiveMissingRequiredFile
                    | UpdateError::ArchiveTopLevelDirectoryMismatch
                    | UpdateError::ArchivePathTraversal
                    | UpdateError::ArchiveDuplicateBinary
            )
        ),
        "unexpected error: {err:?}"
    );

    let envelope = err.error_envelope();
    assert_eq!(
        envelope.error.code,
        "response_contract.update_archive_invalid"
    );
    assert_eq!(source.download_calls(), 2);
    assert_eq!(installer.calls(), 0, "failed validation must not replace");
}

#[test]
fn update_cli_helper_checksum_failure_maps_to_response_contract_cli_error() {
    let platform = current_platform().expect("supported platform for test host");
    let asset = release_asset_name_for_tag("v1.2.3", &platform);
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_download_result(Ok(download_archive_fixture()))
        .with_download_result(Ok(format!(
            "{}  {asset}\n",
            sha256_hex(b"tampered archive")
        )
        .into_bytes()));
    let installer = RecordingInstaller::default();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());

    let err = execute_update_command(
        &source,
        &installer,
        binary_path,
        env!("CARGO_PKG_VERSION").to_string(),
        false,
        true,
        None,
    )
    .expect_err("checksum mismatch must fail");

    assert!(
        matches!(err, CliError::BinaryUpdate(UpdateError::ChecksumMismatch)),
        "unexpected error: {err:?}"
    );

    let envelope = err.error_envelope();
    assert_eq!(
        envelope.error.code,
        "response_contract.update_checksum_invalid"
    );
    assert_eq!(source.download_calls(), 2);
    assert_eq!(installer.calls(), 0, "checksum failure must not replace");
}

#[test]
fn update_cli_helper_release_lookup_failure_maps_to_binary_update_cli_error() {
    let source = FakeSource::new().with_release_result(Err(UpdateError::ReleaseLookupFailed));
    let installer = RecordingInstaller::default();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());

    let err = execute_update_command(
        &source,
        &installer,
        binary_path,
        env!("CARGO_PKG_VERSION").to_string(),
        false,
        true,
        Some("v1.2.3".to_string()),
    )
    .expect_err("release lookup failure must bubble as CliError::BinaryUpdate");

    assert!(
        matches!(
            err,
            CliError::BinaryUpdate(UpdateError::ReleaseLookupFailed)
        ),
        "unexpected error: {err:?}"
    );
    assert_eq!(
        installer.calls(),
        0,
        "failed lookup must not replace binary"
    );

    let envelope = err.error_envelope();
    assert_eq!(envelope.error.code, "api.update_release_lookup_failed");
}

#[test]
fn update_cli_helper_download_failure_maps_to_binary_update_cli_error() {
    let source = FakeSource::new()
        .with_release_result(Ok(
            parse_release_metadata(release_fixture()).expect("release fixture")
        ))
        .with_download_result(Err(UpdateError::AssetDownloadFailed));
    let installer = RecordingInstaller::default();

    let temp = tempdir().expect("tempdir");
    let binary_path = temp.path().join(current_binary_name());

    let err = execute_update_command(
        &source,
        &installer,
        binary_path,
        env!("CARGO_PKG_VERSION").to_string(),
        false,
        true,
        Some("v1.2.3".to_string()),
    )
    .expect_err("download failure must bubble as CliError::BinaryUpdate");

    assert!(
        matches!(
            err,
            CliError::BinaryUpdate(UpdateError::AssetDownloadFailed)
        ),
        "unexpected error: {err:?}"
    );
    assert_eq!(
        source.download_calls(),
        1,
        "download should be attempted once"
    );
    assert_eq!(
        installer.calls(),
        0,
        "failed download must not replace binary"
    );

    let envelope = err.error_envelope();
    assert_eq!(envelope.error.code, "api.update_asset_download_failed");
}

#[derive(Default)]
struct FakeSource {
    release_results: Mutex<VecDeque<Result<ReleaseMetadata, UpdateError>>>,
    download_results: Mutex<VecDeque<Result<Vec<u8>, UpdateError>>>,
    download_calls: Mutex<usize>,
    requested_versions: Mutex<Vec<Option<String>>>,
}

impl FakeSource {
    fn new() -> Self {
        Self::default()
    }

    fn with_release_result(self, result: Result<ReleaseMetadata, UpdateError>) -> Self {
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
        let archive = download_archive_fixture();
        let checksums = checksums_fixture_for_archive(&archive);
        self.with_download_result(Ok(archive))
            .with_download_result(Ok(checksums))
    }

    fn with_archive_and_valid_checksum(self, archive: Vec<u8>) -> Self {
        let checksums = checksums_fixture_for_archive(&archive);
        self.with_download_result(Ok(archive))
            .with_download_result(Ok(checksums))
    }

    fn download_calls(&self) -> usize {
        *self.download_calls.lock().expect("lock")
    }

    fn last_requested_version(&self) -> Option<String> {
        self.requested_versions
            .lock()
            .expect("lock")
            .last()
            .cloned()
            .flatten()
    }
}

impl UpdateSource for FakeSource {
    fn fetch_release(
        &self,
        requested_version: Option<&str>,
    ) -> Result<ReleaseMetadata, UpdateError> {
        self.requested_versions
            .lock()
            .expect("lock")
            .push(requested_version.map(ToString::to_string));

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

#[derive(Default)]
struct RecordingInstaller {
    calls: Mutex<usize>,
    last_bytes: Mutex<Option<Vec<u8>>>,
}

impl RecordingInstaller {
    fn calls(&self) -> usize {
        *self.calls.lock().expect("lock")
    }

    fn last_bytes(&self) -> Option<Vec<u8>> {
        self.last_bytes.lock().expect("lock").clone()
    }
}

impl BinaryInstaller for RecordingInstaller {
    fn replace_binary(
        &self,
        _current_executable: &Path,
        new_binary_bytes: &[u8],
    ) -> Result<(), UpdateError> {
        *self.calls.lock().expect("lock") += 1;
        *self.last_bytes.lock().expect("lock") = Some(new_binary_bytes.to_vec());
        Ok(())
    }
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

fn current_binary_name() -> &'static str {
    current_platform()
        .expect("supported platform for test host")
        .binary_name()
}

fn expected_updated_binary_bytes() -> &'static [u8] {
    let platform = current_platform().expect("supported platform for test host");
    match platform.archive_kind() {
        ArchiveKind::TarGz => b"#!/bin/sh\necho codex-image\n",
        ArchiveKind::Zip => b"MZ",
    }
}

fn download_archive_fixture() -> Vec<u8> {
    let platform = current_platform().expect("supported platform for test host");
    let asset = release_asset_name_for_tag("v1.2.3", &platform);
    let root = match platform.archive_kind() {
        ArchiveKind::TarGz => asset
            .strip_suffix(".tar.gz")
            .expect("tar.gz suffix")
            .to_string(),
        ArchiveKind::Zip => asset.strip_suffix(".zip").expect("zip suffix").to_string(),
    };

    match platform.archive_kind() {
        ArchiveKind::TarGz => tar_gz_fixture(&root, platform.binary_name()),
        ArchiveKind::Zip => zip_fixture(&root, platform.binary_name()),
    }
}

fn checksums_fixture_for_archive(archive: &[u8]) -> Vec<u8> {
    let platform = current_platform().expect("supported platform for test host");
    let asset = release_asset_name_for_tag("v1.2.3", &platform);
    format!("{}  {asset}\n", sha256_hex(archive)).into_bytes()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn tar_gz_fixture(top_level_dir: &str, binary_name: &str) -> Vec<u8> {
    let mut compressed = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut tar = Builder::new(&mut compressed);
        append_tar_file(
            &mut tar,
            &format!("{top_level_dir}/{binary_name}"),
            b"#!/bin/sh\necho codex-image\n",
            0o755,
        );
        append_tar_file(
            &mut tar,
            &format!("{top_level_dir}/README.md"),
            b"README",
            0o644,
        );
        append_tar_file(
            &mut tar,
            &format!("{top_level_dir}/README.ko.md"),
            b"README KO",
            0o644,
        );
        tar.finish().expect("finish tar");
    }

    compressed.finish().expect("finish gzip")
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

fn zip_fixture(top_level_dir: &str, binary_name: &str) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = FileOptions::default();

        zip.start_file(format!("{top_level_dir}/{binary_name}"), options)
            .expect("start binary");
        zip.write_all(b"MZ").expect("write binary");

        zip.start_file(format!("{top_level_dir}/README.md"), options)
            .expect("start readme");
        zip.write_all(b"README").expect("write readme");

        zip.start_file(format!("{top_level_dir}/README.ko.md"), options)
            .expect("start ko readme");
        zip.write_all(b"README KO").expect("write ko readme");

        zip.finish().expect("finish zip");
    }

    cursor.into_inner()
}
