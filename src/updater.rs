use std::env::consts;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{copy, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    TarGz,
    Zip,
}

impl ArchiveKind {
    fn extension(self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
        }
    }

    fn strip_extension(self, name: &str) -> Option<&str> {
        match self {
            Self::TarGz => name.strip_suffix(".tar.gz"),
            Self::Zip => name.strip_suffix(".zip"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    os: &'static str,
    arch: &'static str,
    rust_target: &'static str,
    binary_name: &'static str,
    archive_kind: ArchiveKind,
}

impl Platform {
    pub fn new(os: &str, arch: &str) -> Result<Self, UpdateError> {
        platform_from_parts(os, arch)
    }

    pub fn os(&self) -> &'static str {
        self.os
    }

    pub fn arch(&self) -> &'static str {
        self.arch
    }

    pub fn rust_target(&self) -> &'static str {
        self.rust_target
    }

    pub fn binary_name(&self) -> &'static str {
        self.binary_name
    }

    pub fn archive_kind(&self) -> ArchiveKind {
        self.archive_kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMetadata {
    pub tag_name: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedArtifact {
    pub version: String,
    pub platform_target: String,
    pub asset_name: String,
    pub download_url: String,
    pub checksum_asset_name: String,
    pub checksum_download_url: String,
    pub archive_kind: ArchiveKind,
    pub binary_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedArchive {
    pub binary_path: PathBuf,
    pub binary_archive_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOptions {
    pub current_executable: PathBuf,
    pub current_version: String,
    pub requested_version: Option<String>,
    pub dry_run: bool,
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateResult {
    pub status: String,
    pub current_version: String,
    pub target_version: String,
    pub target: String,
    pub asset: String,
    pub binary_path: String,
}

pub trait UpdateSource {
    fn fetch_release(
        &self,
        requested_version: Option<&str>,
    ) -> Result<ReleaseMetadata, UpdateError>;
    fn download_asset_to_path(
        &self,
        download_url: &str,
        destination: &Path,
    ) -> Result<(), UpdateError>;
}

pub trait BinaryInstaller {
    fn replace_binary(
        &self,
        current_executable: &Path,
        new_binary_bytes: &[u8],
    ) -> Result<(), UpdateError>;
}

pub struct FilesystemBinaryInstaller;

impl BinaryInstaller for FilesystemBinaryInstaller {
    fn replace_binary(
        &self,
        current_executable: &Path,
        new_binary_bytes: &[u8],
    ) -> Result<(), UpdateError> {
        let parent = current_executable
            .parent()
            .ok_or(UpdateError::CurrentExecutableUnavailable)?;

        let file_name = current_executable
            .file_name()
            .ok_or(UpdateError::CurrentExecutableUnavailable)?
            .to_string_lossy();

        let token = unique_token("replace");
        let candidate = parent.join(format!(".{file_name}.{token}.candidate"));
        let backup = parent.join(format!(".{file_name}.{token}.backup"));

        fs::write(&candidate, new_binary_bytes).map_err(|_| UpdateError::ReplacementFailed)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let existing_mode = fs::metadata(current_executable)
                .map(|metadata| metadata.permissions().mode())
                .unwrap_or(0o755);
            let permissions = std::fs::Permissions::from_mode(existing_mode);
            fs::set_permissions(&candidate, permissions)
                .map_err(|_| UpdateError::ReplacementFailed)?;
        }

        fs::rename(current_executable, &backup).map_err(|_| UpdateError::ReplacementFailed)?;

        if fs::rename(&candidate, current_executable).is_err() {
            let _ = fs::rename(&backup, current_executable);
            let _ = fs::remove_file(&candidate);
            return Err(UpdateError::ReplacementFailed);
        }

        let _ = fs::remove_file(&backup);
        Ok(())
    }
}

pub const CHECKSUMS_ASSET_NAME: &str = "SHA256SUMS";

const DEFAULT_GITHUB_API_BASE: &str = "https://api.github.com";
const DEFAULT_GITHUB_CONNECT_TIMEOUT_SECS: u64 = 5;
const DEFAULT_GITHUB_REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitHubClientTimeouts {
    connect_timeout: Duration,
    request_timeout: Duration,
}

impl GitHubClientTimeouts {
    pub fn new(connect_timeout: Duration, request_timeout: Duration) -> Self {
        Self {
            connect_timeout,
            request_timeout,
        }
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }
}

impl Default for GitHubClientTimeouts {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(DEFAULT_GITHUB_CONNECT_TIMEOUT_SECS),
            request_timeout: Duration::from_secs(DEFAULT_GITHUB_REQUEST_TIMEOUT_SECS),
        }
    }
}

pub struct GitHubReleaseClient {
    client: reqwest::blocking::Client,
    repository: String,
    api_base: String,
    timeouts: GitHubClientTimeouts,
}

impl GitHubReleaseClient {
    pub fn new(repository: impl Into<String>) -> Result<Self, UpdateError> {
        Self::with_api_base_and_timeouts(
            repository,
            DEFAULT_GITHUB_API_BASE,
            GitHubClientTimeouts::default(),
        )
    }

    pub fn with_api_base(
        repository: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Result<Self, UpdateError> {
        Self::with_api_base_and_timeouts(repository, api_base, GitHubClientTimeouts::default())
    }

    pub fn with_api_base_and_timeouts(
        repository: impl Into<String>,
        api_base: impl Into<String>,
        timeouts: GitHubClientTimeouts,
    ) -> Result<Self, UpdateError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("codex-image-updater")
            .connect_timeout(timeouts.connect_timeout)
            .timeout(timeouts.request_timeout)
            .build()
            .map_err(|_| UpdateError::ReleaseLookupFailed)?;

        Ok(Self {
            client,
            repository: repository.into(),
            api_base: api_base.into(),
            timeouts,
        })
    }

    pub fn timeouts(&self) -> GitHubClientTimeouts {
        self.timeouts
    }

    fn release_url(&self, requested_version: Option<&str>) -> String {
        match requested_version {
            Some(version) => format!(
                "{}/repos/{}/releases/tags/{}",
                self.api_base, self.repository, version
            ),
            None => format!(
                "{}/repos/{}/releases/latest",
                self.api_base, self.repository
            ),
        }
    }
}

impl UpdateSource for GitHubReleaseClient {
    fn fetch_release(
        &self,
        requested_version: Option<&str>,
    ) -> Result<ReleaseMetadata, UpdateError> {
        let response = self
            .client
            .get(self.release_url(requested_version))
            .send()
            .map_err(|_| UpdateError::ReleaseLookupFailed)?;

        if !response.status().is_success() {
            return Err(UpdateError::ReleaseLookupFailed);
        }

        let text = response
            .text()
            .map_err(|_| UpdateError::ReleaseLookupFailed)?;
        parse_release_metadata(&text)
    }

    fn download_asset_to_path(
        &self,
        download_url: &str,
        destination: &Path,
    ) -> Result<(), UpdateError> {
        let mut response = self
            .client
            .get(download_url)
            .send()
            .map_err(|_| UpdateError::AssetDownloadFailed)?;

        if !response.status().is_success() {
            return Err(UpdateError::AssetDownloadFailed);
        }

        let mut file = File::create(destination).map_err(|_| UpdateError::AssetDownloadFailed)?;
        copy(&mut response, &mut file).map_err(|_| UpdateError::AssetDownloadFailed)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UpdateError {
    #[error("unsupported platform")]
    UnsupportedPlatform,
    #[error("release lookup failed")]
    ReleaseLookupFailed,
    #[error("release metadata is invalid")]
    ReleaseMetadataInvalid,
    #[error("missing required release asset")]
    MissingReleaseAsset,
    #[error("duplicate release asset for platform")]
    DuplicateReleaseAsset,
    #[error("missing SHA256SUMS release asset")]
    MissingChecksumAsset,
    #[error("duplicate SHA256SUMS release asset")]
    DuplicateChecksumAsset,
    #[error("SHA256SUMS metadata is invalid")]
    ChecksumMetadataInvalid,
    #[error("SHA256SUMS is missing the selected archive entry")]
    ChecksumEntryMissing,
    #[error("SHA256SUMS contains duplicate selected archive entries")]
    ChecksumEntryDuplicate,
    #[error("downloaded archive checksum does not match SHA256SUMS")]
    ChecksumMismatch,
    #[error("asset download failed")]
    AssetDownloadFailed,
    #[error("archive payload is invalid")]
    ArchiveInvalid,
    #[error("archive contains path traversal")]
    ArchivePathTraversal,
    #[error("archive top-level directory mismatch")]
    ArchiveTopLevelDirectoryMismatch,
    #[error("archive missing required file")]
    ArchiveMissingRequiredFile,
    #[error("archive contains duplicate binary")]
    ArchiveDuplicateBinary,
    #[error("update confirmation is required")]
    ConfirmationRequired,
    #[error("current executable path is unavailable")]
    CurrentExecutableUnavailable,
    #[error("binary replacement is unsupported on this platform")]
    ReplacementUnsupported,
    #[error("binary replacement failed")]
    ReplacementFailed,
}

pub fn current_platform() -> Result<Platform, UpdateError> {
    platform_from_parts(consts::OS, consts::ARCH)
}

pub fn platform_from_parts(os: &str, arch: &str) -> Result<Platform, UpdateError> {
    match (os, arch) {
        ("linux", "x86_64") => Ok(Platform {
            os: "linux",
            arch: "x86_64",
            rust_target: "x86_64-unknown-linux-gnu",
            binary_name: "codex-image",
            archive_kind: ArchiveKind::TarGz,
        }),
        ("macos", "x86_64") => Ok(Platform {
            os: "macos",
            arch: "x86_64",
            rust_target: "x86_64-apple-darwin",
            binary_name: "codex-image",
            archive_kind: ArchiveKind::TarGz,
        }),
        ("macos", "aarch64") => Ok(Platform {
            os: "macos",
            arch: "aarch64",
            rust_target: "aarch64-apple-darwin",
            binary_name: "codex-image",
            archive_kind: ArchiveKind::TarGz,
        }),
        ("windows", "x86_64") => Ok(Platform {
            os: "windows",
            arch: "x86_64",
            rust_target: "x86_64-pc-windows-msvc",
            binary_name: "codex-image.exe",
            archive_kind: ArchiveKind::Zip,
        }),
        _ => Err(UpdateError::UnsupportedPlatform),
    }
}

pub fn replacement_supported_for_os(os: &str) -> bool {
    os != "windows"
}

pub fn replacement_supported_for_current_os() -> bool {
    replacement_supported_for_os(consts::OS)
}

pub fn release_asset_name_for_tag(tag_name: &str, platform: &Platform) -> String {
    format!(
        "codex-image-{tag_name}-{}.{}",
        platform.rust_target,
        platform.archive_kind.extension()
    )
}

pub fn parse_release_metadata(input: &str) -> Result<ReleaseMetadata, UpdateError> {
    let parsed: GithubRelease =
        serde_json::from_str(input).map_err(|_| UpdateError::ReleaseMetadataInvalid)?;

    if parsed.tag_name.trim().is_empty() {
        return Err(UpdateError::ReleaseMetadataInvalid);
    }

    let assets = parsed
        .assets
        .into_iter()
        .map(|asset| ReleaseAsset {
            name: asset.name,
            download_url: asset.browser_download_url,
        })
        .collect();

    Ok(ReleaseMetadata {
        tag_name: parsed.tag_name,
        assets,
    })
}

pub fn resolve_artifact(
    metadata: &ReleaseMetadata,
    platform: &Platform,
) -> Result<ResolvedArtifact, UpdateError> {
    let expected = release_asset_name_for_tag(&metadata.tag_name, platform);

    let artifact = resolve_unique_asset(
        metadata,
        &expected,
        UpdateError::MissingReleaseAsset,
        UpdateError::DuplicateReleaseAsset,
    )?;
    let checksum_asset = resolve_unique_asset(
        metadata,
        CHECKSUMS_ASSET_NAME,
        UpdateError::MissingChecksumAsset,
        UpdateError::DuplicateChecksumAsset,
    )?;

    Ok(ResolvedArtifact {
        version: metadata.tag_name.clone(),
        platform_target: platform.rust_target.to_string(),
        asset_name: artifact.name.clone(),
        download_url: artifact.download_url.clone(),
        checksum_asset_name: checksum_asset.name.clone(),
        checksum_download_url: checksum_asset.download_url.clone(),
        archive_kind: platform.archive_kind,
        binary_name: platform.binary_name.to_string(),
    })
}

fn resolve_unique_asset<'a>(
    metadata: &'a ReleaseMetadata,
    expected_name: &str,
    missing: UpdateError,
    duplicate: UpdateError,
) -> Result<&'a ReleaseAsset, UpdateError> {
    let mut matches = metadata
        .assets
        .iter()
        .filter(|asset| asset.name == expected_name);
    let first = matches.next().ok_or(missing)?;

    if matches.next().is_some() {
        return Err(duplicate);
    }

    Ok(first)
}

pub fn run_update<S: UpdateSource>(
    source: &S,
    options: &UpdateOptions,
) -> Result<UpdateResult, UpdateError> {
    run_update_with_installer(source, options, &FilesystemBinaryInstaller)
}

pub fn run_update_with_installer<S: UpdateSource, I: BinaryInstaller>(
    source: &S,
    options: &UpdateOptions,
    installer: &I,
) -> Result<UpdateResult, UpdateError> {
    if !options.dry_run && !options.confirm {
        return Err(UpdateError::ConfirmationRequired);
    }

    let platform = current_platform()?;
    let release = source.fetch_release(options.requested_version.as_deref())?;
    let artifact = resolve_artifact(&release, &platform)?;

    let temp_dir = std::env::temp_dir().join(unique_token("codex-image-update"));
    fs::create_dir_all(&temp_dir).map_err(|_| UpdateError::AssetDownloadFailed)?;

    let archive_path = temp_dir.join(&artifact.asset_name);
    let checksum_path = temp_dir.join(&artifact.checksum_asset_name);
    source.download_asset_to_path(&artifact.download_url, &archive_path)?;
    source.download_asset_to_path(&artifact.checksum_download_url, &checksum_path)?;

    let archive_bytes = fs::read(&archive_path).map_err(|_| UpdateError::AssetDownloadFailed)?;
    let checksums =
        fs::read_to_string(&checksum_path).map_err(|_| UpdateError::ChecksumMetadataInvalid)?;
    verify_archive_checksum(&checksums, &artifact.asset_name, &archive_bytes)?;

    let expected_root = expected_archive_root(&artifact)?;
    let validated = validate_archive_bytes(
        artifact.archive_kind,
        &archive_bytes,
        &expected_root,
        &artifact.binary_name,
    )?;

    if options.dry_run {
        return Ok(update_result(options, &artifact, "validated"));
    }

    if !replacement_supported_for_current_os() {
        return Err(UpdateError::ReplacementUnsupported);
    }

    let extracted_binary = extract_binary_bytes(&archive_bytes, artifact.archive_kind, &validated)?;

    installer.replace_binary(&options.current_executable, &extracted_binary)?;

    Ok(update_result(options, &artifact, "updated"))
}

pub fn verify_archive_checksum(
    checksums: &str,
    asset_name: &str,
    archive_bytes: &[u8],
) -> Result<(), UpdateError> {
    let expected = selected_archive_checksum(checksums, asset_name)?;
    let actual = sha256_hex(archive_bytes);

    if actual == expected {
        Ok(())
    } else {
        Err(UpdateError::ChecksumMismatch)
    }
}

pub fn selected_archive_checksum(checksums: &str, asset_name: &str) -> Result<String, UpdateError> {
    if checksums.trim().is_empty() {
        return Err(UpdateError::ChecksumMetadataInvalid);
    }

    let mut selected: Option<String> = None;

    for raw_line in checksums.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let (digest, name) = parse_checksum_line(line)?;

        if name == asset_name {
            if selected.is_some() {
                return Err(UpdateError::ChecksumEntryDuplicate);
            }
            selected = Some(digest.to_ascii_lowercase());
        }
    }

    selected.ok_or(UpdateError::ChecksumEntryMissing)
}

fn parse_checksum_line(line: &str) -> Result<(&str, &str), UpdateError> {
    if line.is_empty() {
        return Err(UpdateError::ChecksumMetadataInvalid);
    }

    let split_at = line
        .find(char::is_whitespace)
        .ok_or(UpdateError::ChecksumMetadataInvalid)?;
    let (digest, remainder) = line.split_at(split_at);

    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdateError::ChecksumMetadataInvalid);
    }

    let remainder = remainder.trim_start_matches(char::is_whitespace);
    let name = remainder.strip_prefix('*').unwrap_or(remainder);

    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return Err(UpdateError::ChecksumMetadataInvalid);
    }

    Ok((digest, name))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn expected_archive_root(artifact: &ResolvedArtifact) -> Result<String, UpdateError> {
    let stem = artifact
        .archive_kind
        .strip_extension(&artifact.asset_name)
        .ok_or(UpdateError::ReleaseMetadataInvalid)?;
    Ok(stem.to_string())
}

fn update_result(
    options: &UpdateOptions,
    artifact: &ResolvedArtifact,
    status: &str,
) -> UpdateResult {
    UpdateResult {
        status: status.to_string(),
        current_version: options.current_version.clone(),
        target_version: artifact.version.clone(),
        target: artifact.platform_target.clone(),
        asset: artifact.asset_name.clone(),
        binary_path: options.current_executable.display().to_string(),
    }
}

fn unique_token(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos}")
}

pub fn validate_archive_bytes(
    archive_kind: ArchiveKind,
    bytes: &[u8],
    expected_top_level_dir: &str,
    expected_binary_name: &str,
) -> Result<ValidatedArchive, UpdateError> {
    match archive_kind {
        ArchiveKind::TarGz => validate_tar_gz_archive(
            bytes,
            expected_top_level_dir,
            expected_binary_name,
            archive_kind,
        ),
        ArchiveKind::Zip => validate_zip_archive(
            bytes,
            expected_top_level_dir,
            expected_binary_name,
            archive_kind,
        ),
    }
}

fn extract_binary_bytes(
    bytes: &[u8],
    archive_kind: ArchiveKind,
    validated: &ValidatedArchive,
) -> Result<Vec<u8>, UpdateError> {
    match archive_kind {
        ArchiveKind::TarGz => extract_from_tar_gz(bytes, &validated.binary_path),
        ArchiveKind::Zip => extract_from_zip(bytes, &validated.binary_archive_path),
    }
}

fn extract_from_tar_gz(bytes: &[u8], validated_binary_path: &Path) -> Result<Vec<u8>, UpdateError> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);

    let entries = archive.entries().map_err(|_| UpdateError::ArchiveInvalid)?;
    for next in entries {
        let mut entry = next.map_err(|_| UpdateError::ArchiveInvalid)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let path = entry.path().map_err(|_| UpdateError::ArchiveInvalid)?;
        if path.as_ref() == validated_binary_path {
            let mut buffer = Vec::new();
            entry
                .read_to_end(&mut buffer)
                .map_err(|_| UpdateError::ArchiveInvalid)?;
            return Ok(buffer);
        }
    }

    Err(UpdateError::ArchiveMissingRequiredFile)
}

fn extract_from_zip(
    bytes: &[u8],
    validated_binary_archive_path: &str,
) -> Result<Vec<u8>, UpdateError> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|_| UpdateError::ArchiveInvalid)?;

    let mut file = archive
        .by_name(validated_binary_archive_path)
        .map_err(|_| UpdateError::ArchiveMissingRequiredFile)?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|_| UpdateError::ArchiveInvalid)?;
    Ok(buffer)
}

fn validate_tar_gz_archive(
    bytes: &[u8],
    expected_top_level_dir: &str,
    expected_binary_name: &str,
    archive_kind: ArchiveKind,
) -> Result<ValidatedArchive, UpdateError> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);

    let entries = archive.entries().map_err(|_| UpdateError::ArchiveInvalid)?;
    let mut state = ValidationState::new(archive_kind, expected_binary_name);

    for next in entries {
        let mut entry = next.map_err(|_| UpdateError::ArchiveInvalid)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let path = entry.path().map_err(|_| UpdateError::ArchiveInvalid)?;
        state.observe_path(path.as_ref(), expected_top_level_dir)?;

        let mut sink = Vec::new();
        entry
            .read_to_end(&mut sink)
            .map_err(|_| UpdateError::ArchiveInvalid)?;
    }

    state.finish()
}

fn validate_zip_archive(
    bytes: &[u8],
    expected_top_level_dir: &str,
    expected_binary_name: &str,
    archive_kind: ArchiveKind,
) -> Result<ValidatedArchive, UpdateError> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|_| UpdateError::ArchiveInvalid)?;

    let mut state = ValidationState::new(archive_kind, expected_binary_name);

    for idx in 0..archive.len() {
        let mut file = archive
            .by_index(idx)
            .map_err(|_| UpdateError::ArchiveInvalid)?;
        if file.is_dir() {
            continue;
        }

        let path = Path::new(file.name());
        state.observe_path(path, expected_top_level_dir)?;

        let mut sink = Vec::new();
        file.read_to_end(&mut sink)
            .map_err(|_| UpdateError::ArchiveInvalid)?;
    }

    state.finish()
}

struct ValidationState {
    archive_kind: ArchiveKind,
    expected_binary_name: String,
    binary_path: Option<PathBuf>,
    binary_archive_path: Option<String>,
    has_readme: bool,
    has_readme_ko: bool,
}

impl ValidationState {
    fn new(archive_kind: ArchiveKind, expected_binary_name: &str) -> Self {
        Self {
            archive_kind,
            expected_binary_name: expected_binary_name.to_string(),
            binary_path: None,
            binary_archive_path: None,
            has_readme: false,
            has_readme_ko: false,
        }
    }

    fn observe_path(
        &mut self,
        path: &Path,
        expected_top_level_dir: &str,
    ) -> Result<(), UpdateError> {
        let relative = normalize_archive_path(path, expected_top_level_dir)?;

        if relative == Path::new("README.md") {
            self.has_readme = true;
        }

        if relative == Path::new("README.ko.md") {
            self.has_readme_ko = true;
        }

        let file_name = relative.file_name().and_then(|value| value.to_str());
        if file_name == Some(self.expected_binary_name.as_str()) {
            let normalized = Path::new(expected_top_level_dir).join(relative);
            let archive_name = path_to_archive_name(&normalized)?;
            if self.binary_path.is_some() {
                return Err(UpdateError::ArchiveDuplicateBinary);
            }
            self.binary_path = Some(normalized);
            self.binary_archive_path = Some(archive_name);
        }

        Ok(())
    }

    fn finish(self) -> Result<ValidatedArchive, UpdateError> {
        if !self.has_readme || !self.has_readme_ko {
            return Err(UpdateError::ArchiveMissingRequiredFile);
        }

        let Some(binary_path) = self.binary_path else {
            return Err(UpdateError::ArchiveMissingRequiredFile);
        };

        let Some(binary_archive_path) = self.binary_archive_path else {
            return Err(UpdateError::ArchiveMissingRequiredFile);
        };

        let _ = self.archive_kind;

        Ok(ValidatedArchive {
            binary_path,
            binary_archive_path,
        })
    }
}

fn path_to_archive_name(path: &Path) -> Result<String, UpdateError> {
    let mut archive_name = String::new();

    for component in path.components() {
        match component {
            Component::Normal(value) => {
                if !archive_name.is_empty() {
                    archive_name.push('/');
                }
                archive_name.push_str(&value.to_string_lossy());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(UpdateError::ArchivePathTraversal)
            }
        }
    }

    if archive_name.is_empty() {
        return Err(UpdateError::ArchiveInvalid);
    }

    Ok(archive_name)
}

fn normalize_archive_path(
    path: &Path,
    expected_top_level_dir: &str,
) -> Result<PathBuf, UpdateError> {
    let mut components = path.components();

    let Some(first) = components.next() else {
        return Err(UpdateError::ArchiveInvalid);
    };

    let top_level = match first {
        Component::Normal(value) => value,
        Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
            return Err(UpdateError::ArchivePathTraversal)
        }
    };

    if top_level != expected_top_level_dir {
        return Err(UpdateError::ArchiveTopLevelDirectoryMismatch);
    }

    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(UpdateError::ArchivePathTraversal)
            }
        }
    }

    if relative.as_os_str().is_empty() {
        return Err(UpdateError::ArchiveInvalid);
    }

    Ok(relative)
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}
