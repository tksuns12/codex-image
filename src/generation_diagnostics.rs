use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;

use crate::diagnostics::CliError;

pub const GENERATION_DIAGNOSTICS_SCHEMA: &str = "codex-image.generation-diagnostics";
pub const GENERATION_DIAGNOSTICS_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GenerationDiagnostics {
    pub schema: &'static str,
    pub schema_version: u8,
    pub metadata: GenerationDiagnosticsMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<BatchDiagnosticsSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<RedactedFailure>,
    pub runs: Vec<CodexRunDiagnostics>,
    pub redaction: RedactionMetadata,
}

impl GenerationDiagnostics {
    pub fn new(metadata: GenerationDiagnosticsMetadata) -> Self {
        Self {
            schema: GENERATION_DIAGNOSTICS_SCHEMA,
            schema_version: GENERATION_DIAGNOSTICS_SCHEMA_VERSION,
            metadata,
            batch: None,
            failure: None,
            runs: Vec::new(),
            redaction: RedactionMetadata::default(),
        }
    }

    pub fn with_batch_summary(mut self, batch: BatchDiagnosticsSummary) -> Self {
        self.batch = Some(batch);
        self
    }

    pub fn with_failure(mut self, failure: &CliError) -> Self {
        self.failure = Some(RedactedFailure::from_cli_error(failure));
        self
    }

    pub fn with_runs(mut self, runs: Vec<CodexRunDiagnostics>) -> Self {
        self.runs = runs;
        self
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GenerationDiagnosticsMetadata {
    pub generated_at_unix_seconds: i64,
    pub mode: GenerationDiagnosticsMode,
    pub result: GenerationDiagnosticsResult,
    pub prompt_source: PromptSourceKind,
    pub stdout_mode: GenerationDiagnosticsStdoutMode,
    pub timeout_seconds: u64,
    pub command: SanitizedCommand,
}

impl GenerationDiagnosticsMetadata {
    pub fn for_invocation(
        mode: GenerationDiagnosticsMode,
        result: GenerationDiagnosticsResult,
        prompt_source: PromptSourceKind,
        stdout_mode: GenerationDiagnosticsStdoutMode,
        timeout_seconds: u64,
    ) -> Self {
        Self::for_invocation_at(
            Utc::now().timestamp(),
            mode,
            result,
            prompt_source,
            stdout_mode,
            timeout_seconds,
        )
    }

    pub fn for_invocation_at(
        generated_at_unix_seconds: i64,
        mode: GenerationDiagnosticsMode,
        result: GenerationDiagnosticsResult,
        prompt_source: PromptSourceKind,
        stdout_mode: GenerationDiagnosticsStdoutMode,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            generated_at_unix_seconds,
            mode,
            result,
            prompt_source,
            stdout_mode,
            timeout_seconds,
            command: SanitizedCommand::codex_subprocess(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationDiagnosticsMode {
    Single,
    Batch,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationDiagnosticsResult {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptSourceKind {
    PositionalPrompt,
    PromptFile,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationDiagnosticsStdoutMode {
    Human,
    Json,
    Quiet,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SanitizedCommand {
    pub program: &'static str,
    pub args: Vec<&'static str>,
}

impl SanitizedCommand {
    pub fn codex_subprocess() -> Self {
        Self {
            program: "codex",
            args: vec![
                "exec",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "-C",
                "[working-directory]",
                "--output-last-message",
                "[last-message-file]",
                "--color",
                "never",
                "[image-generation-prompt]",
            ],
        }
    }

    pub fn codex_image_generation() -> Self {
        Self::codex_subprocess()
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct BatchDiagnosticsSummary {
    pub planned_items: usize,
    pub completed_items: usize,
    pub failed_item_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodexRunDiagnostics {
    pub index: usize,
    pub item_index: Option<usize>,
    pub phase: CodexRunPhase,
    pub outcome: CodexRunOutcome,
    pub status: CodexRunStatus,
    pub timeout_seconds: u64,
    pub timed_out: bool,
    pub elapsed_millis: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub command: SanitizedCommand,
    pub final_message: FinalMessageDiagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<RedactedFailure>,
}

impl CodexRunDiagnostics {
    pub fn success(
        index: usize,
        item_index: Option<usize>,
        timeout_seconds: u64,
        elapsed_millis: u64,
        exit_code: i32,
        final_message: FinalMessageDiagnostics,
    ) -> Self {
        Self {
            index,
            item_index,
            phase: CodexRunPhase::GenerateImage,
            outcome: CodexRunOutcome::Succeeded,
            status: CodexRunStatus::Succeeded,
            timeout_seconds,
            timed_out: false,
            elapsed_millis,
            exit_code: Some(exit_code),
            command: SanitizedCommand::codex_subprocess(),
            final_message,
            failure: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn failure(
        index: usize,
        item_index: Option<usize>,
        timeout_seconds: u64,
        elapsed_millis: u64,
        exit_code: Option<i32>,
        status: CodexRunStatus,
        final_message: FinalMessageDiagnostics,
        failure: &CliError,
    ) -> Self {
        let timed_out = status == CodexRunStatus::TimedOut;
        Self {
            index,
            item_index,
            phase: CodexRunPhase::GenerateImage,
            outcome: if timed_out {
                CodexRunOutcome::TimedOut
            } else {
                CodexRunOutcome::Failed
            },
            status,
            timeout_seconds,
            timed_out,
            elapsed_millis,
            exit_code,
            command: SanitizedCommand::codex_subprocess(),
            final_message,
            failure: Some(RedactedFailure::from_cli_error(failure)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexRunPhase {
    GenerateImage,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexRunOutcome {
    Succeeded,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexRunStatus {
    NotStarted,
    Succeeded,
    Failed,
    TimedOut,
    FinalMessageInvalid,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FinalMessageDiagnostics {
    pub status: FinalMessageStatus,
    pub presence: FinalMessagePresence,
    pub parse: FinalMessageParseStatus,
    #[serde(rename = "image_path_status")]
    pub image_path: FinalMessageFieldStatus,
    #[serde(rename = "note_status")]
    pub note: FinalMessageFieldStatus,
}

impl FinalMessageDiagnostics {
    pub fn not_observed() -> Self {
        Self {
            status: FinalMessageStatus::NotObserved,
            presence: FinalMessagePresence::NotObserved,
            parse: FinalMessageParseStatus::NotAttempted,
            image_path: FinalMessageFieldStatus::NotObserved,
            note: FinalMessageFieldStatus::NotObserved,
        }
    }

    pub fn missing() -> Self {
        Self {
            status: FinalMessageStatus::Missing,
            presence: FinalMessagePresence::Missing,
            parse: FinalMessageParseStatus::NotAttempted,
            image_path: FinalMessageFieldStatus::NotObserved,
            note: FinalMessageFieldStatus::NotObserved,
        }
    }

    pub fn invalid_json() -> Self {
        Self {
            status: FinalMessageStatus::InvalidJson,
            presence: FinalMessagePresence::Present,
            parse: FinalMessageParseStatus::InvalidJson,
            image_path: FinalMessageFieldStatus::NotObserved,
            note: FinalMessageFieldStatus::NotObserved,
        }
    }

    pub fn contract_invalid(
        image_path: FinalMessageFieldStatus,
        note: FinalMessageFieldStatus,
    ) -> Self {
        Self {
            status: FinalMessageStatus::ContractInvalid,
            presence: FinalMessagePresence::Present,
            parse: FinalMessageParseStatus::ContractInvalid,
            image_path,
            note,
        }
    }

    pub fn parsed(note: FinalMessageFieldStatus) -> Self {
        Self {
            status: FinalMessageStatus::Parsed,
            presence: FinalMessagePresence::Present,
            parse: FinalMessageParseStatus::Parsed,
            image_path: FinalMessageFieldStatus::Present,
            note,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalMessageStatus {
    NotObserved,
    Missing,
    InvalidJson,
    ContractInvalid,
    Parsed,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalMessagePresence {
    NotObserved,
    Missing,
    Present,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalMessageParseStatus {
    NotAttempted,
    InvalidJson,
    ContractInvalid,
    Parsed,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalMessageFieldStatus {
    NotObserved,
    Missing,
    Present,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RedactedFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub recoverable: bool,
    pub hint: &'static str,
}

impl RedactedFailure {
    pub fn from_cli_error(error: &CliError) -> Self {
        let details = error.error_envelope().error;
        Self {
            code: details.code,
            message: details.message,
            recoverable: details.recoverable,
            hint: details.hint,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RedactionMetadata {
    pub prompt_text: &'static str,
    pub prompt_hashes: &'static str,
    pub filesystem_locations: &'static str,
    pub codex_streams: &'static str,
    pub final_message_payload: &'static str,
    pub credentials: &'static str,
    pub image_payloads: &'static str,
}

impl Default for RedactionMetadata {
    fn default() -> Self {
        Self {
            prompt_text: "omitted",
            prompt_hashes: "omitted",
            filesystem_locations: "placeholder-only",
            codex_streams: "omitted",
            final_message_payload: "status-only",
            credentials: "omitted",
            image_payloads: "omitted",
        }
    }
}

pub fn write_generation_diagnostics(
    path: &Path,
    diagnostics: &GenerationDiagnostics,
) -> Result<(), CliError> {
    let bytes = serde_json::to_vec_pretty(diagnostics).map_err(|_| CliError::OutputWriteFailed)?;
    atomic_write_bytes(path, &bytes).map_err(|_| CliError::OutputWriteFailed)
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = parent_dir(path)?;
    fs::create_dir_all(&parent)?;

    let mut attempt = 0_u32;
    let pid = std::process::id();

    loop {
        let tmp_path = temp_path(path, &parent, pid, attempt)?;
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
        {
            Ok(file) => {
                if let Err(err) = write_and_rename(file, &tmp_path, path, bytes) {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(err);
                }
                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt += 1;
                if attempt > 100 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "failed to allocate temporary diagnostics file",
                    ));
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn parent_dir(path: &Path) -> std::io::Result<PathBuf> {
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            Ok(PathBuf::from("."))
        } else {
            Ok(parent.to_path_buf())
        }
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "diagnostics path does not have a parent directory",
        ))
    }
}

fn temp_path(path: &Path, parent: &Path, pid: u32, attempt: u32) -> std::io::Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "diagnostics path does not have a file name",
            )
        })?;

    Ok(parent.join(format!(".{file_name}.tmp-{pid}-{attempt}")))
}

fn write_and_rename(
    mut file: fs::File,
    tmp_path: &Path,
    final_path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, final_path)
}
