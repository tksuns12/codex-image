use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::skills::{resolve_skill_path, SkillScope, SupportedTool};

const MANAGED_MARKER_PREFIX: &str = "<!-- codex-image:managed checksum=";
const MANAGED_MARKER_SUFFIX: &str = " -->";

const SKILL_BODY: &str = include_str!("templates/codex-image-skill.md");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillContentClassification {
    Missing,
    ManagedCurrent,
    ManagedOutdated,
    ManualUnmanaged,
    ManagedTampered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedLayout {
    LegacyPrefix,
    Suffix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedManagedContent {
    checksum: String,
    body: String,
    layout: ManagedLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInstallPlan {
    tool: SupportedTool,
    scope: SkillScope,
    target_path: PathBuf,
}

impl SkillInstallPlan {
    pub fn build(
        tool: SupportedTool,
        scope: SkillScope,
        home_dir: &Path,
        project_root: &Path,
    ) -> Self {
        let target_path = resolve_skill_path(tool, scope, home_dir, project_root);
        Self {
            tool,
            scope,
            target_path,
        }
    }

    pub fn tool(&self) -> SupportedTool {
        self.tool
    }

    pub fn scope(&self) -> SkillScope {
        self.scope
    }

    pub fn target_path(&self) -> &Path {
        &self.target_path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkillInstallOptions {
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillInstallStatus {
    Created,
    Unchanged,
    Updated,
    BlockedManualEdit,
    ForcedOverwrite,
}

impl SkillInstallStatus {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Unchanged => "unchanged",
            Self::Updated => "updated",
            Self::BlockedManualEdit => "blocked_manual_edit",
            Self::ForcedOverwrite => "forced_overwrite",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInstallResult {
    pub status: SkillInstallStatus,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillUninstallStatus {
    Deleted,
    AlreadyMissing,
    BlockedManualEdit,
    ForcedDelete,
}

impl SkillUninstallStatus {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Deleted => "deleted",
            Self::AlreadyMissing => "already_missing",
            Self::BlockedManualEdit => "blocked_manual_edit",
            Self::ForcedDelete => "forced_delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillUninstallResult {
    pub status: SkillUninstallStatus,
    pub path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillInstallError {
    #[error("failed to read existing skill file")]
    ReadFailed,
    #[error("failed to create parent directory")]
    CreateParentDirFailed,
    #[error("failed to write skill file")]
    WriteFailed,
    #[error("failed to rename temporary skill file")]
    RenameFailed,
    #[error("skill path has no parent directory")]
    MissingParentDirectory,
}

pub fn install_skill(
    plan: &SkillInstallPlan,
    options: SkillInstallOptions,
) -> Result<SkillInstallResult, SkillInstallError> {
    let existing_content = read_existing_skill(plan.target_path())?;
    let classification = classify_skill_content(existing_content.as_deref());

    let desired_content = render_managed_skill_content();

    let status = match classification {
        SkillContentClassification::Missing => {
            write_managed_skill_file(plan.target_path(), &desired_content)?;
            SkillInstallStatus::Created
        }
        SkillContentClassification::ManagedCurrent => SkillInstallStatus::Unchanged,
        SkillContentClassification::ManagedOutdated => {
            write_managed_skill_file(plan.target_path(), &desired_content)?;
            SkillInstallStatus::Updated
        }
        SkillContentClassification::ManualUnmanaged
        | SkillContentClassification::ManagedTampered => {
            if options.force {
                write_managed_skill_file(plan.target_path(), &desired_content)?;
                SkillInstallStatus::ForcedOverwrite
            } else {
                SkillInstallStatus::BlockedManualEdit
            }
        }
    };

    Ok(SkillInstallResult {
        status,
        path: plan.target_path().to_path_buf(),
    })
}

pub fn uninstall_skill(
    plan: &SkillInstallPlan,
    options: SkillInstallOptions,
) -> Result<SkillUninstallResult, SkillInstallError> {
    let classification = classify_skill_path(plan.target_path())?;

    let status = match classification {
        SkillContentClassification::Missing => SkillUninstallStatus::AlreadyMissing,
        SkillContentClassification::ManagedCurrent
        | SkillContentClassification::ManagedOutdated => {
            remove_skill_file(plan.target_path())?;
            SkillUninstallStatus::Deleted
        }
        SkillContentClassification::ManualUnmanaged
        | SkillContentClassification::ManagedTampered => {
            if options.force {
                remove_skill_file(plan.target_path())?;
                SkillUninstallStatus::ForcedDelete
            } else {
                SkillUninstallStatus::BlockedManualEdit
            }
        }
    };

    Ok(SkillUninstallResult {
        status,
        path: plan.target_path().to_path_buf(),
    })
}

pub fn render_skill_body() -> &'static str {
    SKILL_BODY
}

pub fn managed_checksum(body: &str) -> String {
    // Deterministic FNV-1a 64-bit checksum over UTF-8 bytes.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in body.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{hash:016x}")
}

pub fn managed_marker_line(body: &str) -> String {
    format!(
        "{MANAGED_MARKER_PREFIX}{}{MANAGED_MARKER_SUFFIX}",
        managed_checksum(body)
    )
}

pub fn render_managed_skill_content() -> String {
    let body = render_skill_body();
    let separator = if body.ends_with('\n') { "" } else { "\n" };

    format!("{body}{separator}{}\n", managed_marker_line(body))
}

pub fn classify_skill_content(existing_content: Option<&str>) -> SkillContentClassification {
    let Some(existing_content) = existing_content else {
        return SkillContentClassification::Missing;
    };

    let expected_body = render_skill_body();
    let Some(parsed) = split_managed_content(existing_content) else {
        if has_codex_image_marker_prefix(existing_content) {
            return SkillContentClassification::ManagedTampered;
        }
        return SkillContentClassification::ManualUnmanaged;
    };

    let computed = managed_checksum(&parsed.body);
    if computed != parsed.checksum {
        return SkillContentClassification::ManagedTampered;
    }

    if parsed.body == expected_body {
        match parsed.layout {
            ManagedLayout::Suffix => SkillContentClassification::ManagedCurrent,
            ManagedLayout::LegacyPrefix => SkillContentClassification::ManagedOutdated,
        }
    } else {
        SkillContentClassification::ManagedOutdated
    }
}

pub fn classify_skill_path(path: &Path) -> Result<SkillContentClassification, SkillInstallError> {
    let existing_content = read_existing_skill(path)?;
    Ok(classify_skill_content(existing_content.as_deref()))
}

fn read_existing_skill(path: &Path) -> Result<Option<String>, SkillInstallError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(SkillInstallError::ReadFailed),
    }
}

fn write_managed_skill_file(path: &Path, content: &str) -> Result<(), SkillInstallError> {
    let Some(parent) = path.parent() else {
        return Err(SkillInstallError::MissingParentDirectory);
    };

    fs::create_dir_all(parent).map_err(|_| SkillInstallError::CreateParentDirFailed)?;

    let tmp_file = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("SKILL.md")
    ));

    fs::write(&tmp_file, content).map_err(|_| SkillInstallError::WriteFailed)?;

    fs::rename(&tmp_file, path).map_err(|_| {
        let _ = fs::remove_file(&tmp_file);
        SkillInstallError::RenameFailed
    })
}

fn remove_skill_file(path: &Path) -> Result<(), SkillInstallError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(SkillInstallError::WriteFailed),
    }
}

fn split_managed_content(content: &str) -> Option<ParsedManagedContent> {
    // Legacy format (M002/S02-S04): marker first, then body.
    if let Some((first_line, remainder_with_newline)) = content.split_once('\n') {
        if let Some(checksum) = parse_managed_marker_line(first_line) {
            return Some(ParsedManagedContent {
                checksum: checksum.to_string(),
                body: remainder_with_newline.to_string(),
                layout: ManagedLayout::LegacyPrefix,
            });
        }
    }

    // Current format: body first, marker last so frontmatter starts at line 1.
    let trimmed = content.trim_end_matches(['\n', '\r']);
    let (body_prefix, last_line) = trimmed.rsplit_once('\n')?;
    let checksum = parse_managed_marker_line(last_line)?;

    // Body should retain a trailing newline for stable checksum compatibility.
    let mut body = body_prefix.to_string();
    body.push('\n');

    Some(ParsedManagedContent {
        checksum: checksum.to_string(),
        body,
        layout: ManagedLayout::Suffix,
    })
}

fn parse_managed_marker_line(line: &str) -> Option<&str> {
    let checksum = line
        .strip_prefix(MANAGED_MARKER_PREFIX)?
        .strip_suffix(MANAGED_MARKER_SUFFIX)?;

    if !is_valid_checksum_hex(checksum) {
        return None;
    }

    Some(checksum)
}

fn has_codex_image_marker_prefix(content: &str) -> bool {
    content.lines().next().is_some_and(|line| {
        line.starts_with("<!-- codex-image:managed")
            || line.starts_with(MANAGED_MARKER_PREFIX)
            || line.contains("codex-image:managed")
    })
}

fn is_valid_checksum_hex(value: &str) -> bool {
    value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
