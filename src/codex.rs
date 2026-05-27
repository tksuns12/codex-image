use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::config::{read_non_empty_env_path, ENV_CODEX_BIN};
use crate::diagnostics::CliError;
use crate::generation_diagnostics::{
    CodexRunDiagnostics, CodexRunStatus, FinalMessageDiagnostics, FinalMessageFieldStatus,
};

pub const DEFAULT_CODEX_EXEC_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_CODEX_EXEC_TIMEOUT: Duration =
    Duration::from_secs(DEFAULT_CODEX_EXEC_TIMEOUT_SECS);

#[derive(Debug, Clone)]
pub struct CodexGenerationOptions {
    pub timeout: Duration,
    pub diagnostics: Option<CodexDiagnosticsRecorder>,
}

impl CodexGenerationOptions {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            diagnostics: None,
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: CodexDiagnosticsRecorder) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
}

impl Default for CodexGenerationOptions {
    fn default() -> Self {
        Self::new(DEFAULT_CODEX_EXEC_TIMEOUT)
    }
}

#[derive(Debug, Clone)]
pub struct CodexDiagnosticsRecorder {
    runs: Arc<Mutex<Vec<CodexRunDiagnostics>>>,
    item_index: Option<usize>,
}

impl CodexDiagnosticsRecorder {
    pub fn new(item_index: Option<usize>) -> Self {
        Self {
            runs: Arc::new(Mutex::new(Vec::new())),
            item_index,
        }
    }

    pub fn for_item(&self, item_index: usize) -> Self {
        Self {
            runs: Arc::clone(&self.runs),
            item_index: Some(item_index),
        }
    }

    pub fn runs(&self) -> Vec<CodexRunDiagnostics> {
        self.runs
            .lock()
            .map(|runs| runs.clone())
            .unwrap_or_default()
    }

    fn record(&self, build: impl FnOnce(usize, Option<usize>) -> CodexRunDiagnostics) {
        if let Ok(mut runs) = self.runs.lock() {
            let index = runs.len() + 1;
            runs.push(build(index, self.item_index));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexImageGeneration {
    pub source_path: PathBuf,
    pub source_not_before: SystemTime,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexFinalMessage {
    image_path: String,
    #[serde(default)]
    note: Option<String>,
}

struct ParsedFinalMessage {
    message: CodexFinalMessage,
    diagnostics: FinalMessageDiagnostics,
}

struct FinalMessageParseFailure {
    error: CliError,
    diagnostics: FinalMessageDiagnostics,
}

pub fn generate_image_with_codex(
    prompt: &str,
    out_dir: &Path,
) -> Result<CodexImageGeneration, CliError> {
    generate_image_with_codex_with_options(prompt, out_dir, CodexGenerationOptions::default())
}

pub fn generate_image_with_codex_with_options(
    prompt: &str,
    out_dir: &Path,
    options: CodexGenerationOptions,
) -> Result<CodexImageGeneration, CliError> {
    fs::create_dir_all(out_dir).map_err(|_| CliError::OutputWriteFailed)?;

    let timeout_seconds = options.timeout.as_secs();
    let codex_bin = resolve_codex_binary()?;
    let last_message_path = out_dir.join(format!(
        ".codex-image-last-message-{}.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&last_message_path);

    let codex_prompt = build_codex_prompt(prompt);
    let source_not_before = current_filesystem_timestamp_floor();
    let mut child = match Command::new(&codex_bin)
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg("read-only")
        .arg("-C")
        .arg(out_dir)
        .arg("--output-last-message")
        .arg(&last_message_path)
        .arg("--color")
        .arg("never")
        .arg(codex_prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            let err = CliError::CodexImageGenerationFailed {
                source_message: "failed to spawn Codex CLI".to_string(),
            };
            record_codex_run(&options, |index, item_index| {
                CodexRunDiagnostics::failure(
                    index,
                    item_index,
                    timeout_seconds,
                    0,
                    None,
                    CodexRunStatus::Failed,
                    FinalMessageDiagnostics::not_observed(),
                    &err,
                )
            });
            return Err(err);
        }
    };

    let wait_outcome = child.wait_timeout(options.timeout);
    let (status, elapsed_millis) = match wait_outcome {
        ChildWaitOutcome::Completed {
            status,
            elapsed_millis,
        } => (status, elapsed_millis),
        ChildWaitOutcome::TimedOut { elapsed_millis } => {
            let _ = fs::remove_file(&last_message_path);
            let err = CliError::CodexImageGenerationFailed {
                source_message: "Codex CLI image generation timed out".to_string(),
            };
            record_codex_run(&options, |index, item_index| {
                CodexRunDiagnostics::failure(
                    index,
                    item_index,
                    timeout_seconds,
                    elapsed_millis,
                    None,
                    CodexRunStatus::TimedOut,
                    FinalMessageDiagnostics::not_observed(),
                    &err,
                )
            });
            return Err(err);
        }
        ChildWaitOutcome::WaitFailed { elapsed_millis } => {
            let _ = fs::remove_file(&last_message_path);
            let err = CliError::CodexImageGenerationFailed {
                source_message: "failed to wait for Codex CLI".to_string(),
            };
            record_codex_run(&options, |index, item_index| {
                CodexRunDiagnostics::failure(
                    index,
                    item_index,
                    timeout_seconds,
                    elapsed_millis,
                    None,
                    CodexRunStatus::Failed,
                    FinalMessageDiagnostics::not_observed(),
                    &err,
                )
            });
            return Err(err);
        }
    };

    if !status.success() {
        let _ = fs::remove_file(&last_message_path);
        let err = CliError::CodexImageGenerationFailed {
            source_message: format!("Codex CLI exited with status {status}"),
        };
        record_codex_run(&options, |index, item_index| {
            CodexRunDiagnostics::failure(
                index,
                item_index,
                timeout_seconds,
                elapsed_millis,
                status.code(),
                CodexRunStatus::Failed,
                FinalMessageDiagnostics::not_observed(),
                &err,
            )
        });
        return Err(err);
    }

    let final_message = match fs::read_to_string(&last_message_path) {
        Ok(final_message) => final_message,
        Err(_) => {
            let _ = fs::remove_file(&last_message_path);
            let err = CliError::ImageGenerationResponseContract {
                source_message: "Codex CLI did not write final image JSON".to_string(),
            };
            record_codex_run(&options, |index, item_index| {
                CodexRunDiagnostics::failure(
                    index,
                    item_index,
                    timeout_seconds,
                    elapsed_millis,
                    status.code(),
                    CodexRunStatus::FinalMessageInvalid,
                    FinalMessageDiagnostics::missing(),
                    &err,
                )
            });
            return Err(err);
        }
    };
    let _ = fs::remove_file(&last_message_path);

    let parsed = match parse_final_message(&final_message) {
        Ok(parsed) => parsed,
        Err(failure) => {
            record_codex_run(&options, |index, item_index| {
                CodexRunDiagnostics::failure(
                    index,
                    item_index,
                    timeout_seconds,
                    elapsed_millis,
                    status.code(),
                    CodexRunStatus::FinalMessageInvalid,
                    failure.diagnostics,
                    &failure.error,
                )
            });
            return Err(failure.error);
        }
    };

    let source_path = PathBuf::from(&parsed.message.image_path);
    if !source_path.is_file() {
        let err = CliError::ImageGenerationResponseContract {
            source_message: "Codex image path does not exist".to_string(),
        };
        record_codex_run(&options, |index, item_index| {
            CodexRunDiagnostics::failure(
                index,
                item_index,
                timeout_seconds,
                elapsed_millis,
                status.code(),
                CodexRunStatus::FinalMessageInvalid,
                parsed.diagnostics.clone(),
                &err,
            )
        });
        return Err(err);
    }

    record_codex_run(&options, |index, item_index| {
        CodexRunDiagnostics::success(
            index,
            item_index,
            timeout_seconds,
            elapsed_millis,
            status.code().unwrap_or(0),
            parsed.diagnostics.clone(),
        )
    });

    Ok(CodexImageGeneration {
        source_path,
        source_not_before,
        note: parsed.message.note,
    })
}

fn record_codex_run(
    options: &CodexGenerationOptions,
    build: impl FnOnce(usize, Option<usize>) -> CodexRunDiagnostics,
) {
    if let Some(recorder) = &options.diagnostics {
        recorder.record(build);
    }
}

fn current_filesystem_timestamp_floor() -> SystemTime {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    UNIX_EPOCH + Duration::from_secs(since_epoch.as_secs())
}

fn build_codex_prompt(prompt: &str) -> String {
    format!(
        r#"Generate exactly one raster image using Codex's built-in image generation tool.
Do not use OPENAI_API_KEY, the Image API fallback CLI, curl, Python API clients, or browser automation.
Do not copy the generated image into the workspace; just locate the image file produced by the built-in tool.

User image prompt:
{prompt}

Final answer requirements:
Return exactly one JSON object and no markdown fences, prose, or extra text.
The JSON object must have this shape:
{{"image_path":"/absolute/path/to/generated/image.png","note":"short status note"}}
"#
    )
}

fn parse_final_message(message: &str) -> Result<ParsedFinalMessage, FinalMessageParseFailure> {
    let value = parse_final_message_value(message)?;
    let image_path = field_status(value.get("image_path"));
    let note = field_status(value.get("note"));

    let parsed = serde_json::from_value::<CodexFinalMessage>(value).map_err(|_| {
        FinalMessageParseFailure {
            error: CliError::ImageGenerationResponseContract {
                source_message: "Codex final image JSON did not match expected schema".to_string(),
            },
            diagnostics: FinalMessageDiagnostics::contract_invalid(image_path, note),
        }
    })?;

    let note = if parsed.note.is_some() {
        FinalMessageFieldStatus::Present
    } else {
        FinalMessageFieldStatus::Missing
    };

    Ok(ParsedFinalMessage {
        message: parsed,
        diagnostics: FinalMessageDiagnostics::parsed(note),
    })
}

fn parse_final_message_value(message: &str) -> Result<serde_json::Value, FinalMessageParseFailure> {
    let trimmed = message.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(value);
    }

    let start = message.find('{').ok_or_else(invalid_final_message_json)?;
    let end = message.rfind('}').ok_or_else(invalid_final_message_json)?;
    if start > end {
        return Err(invalid_final_message_json());
    }

    serde_json::from_str::<serde_json::Value>(&message[start..=end])
        .map_err(|_| invalid_final_message_json())
}

fn invalid_final_message_json() -> FinalMessageParseFailure {
    FinalMessageParseFailure {
        error: CliError::ImageGenerationResponseContract {
            source_message: "Codex final message did not contain valid JSON".to_string(),
        },
        diagnostics: FinalMessageDiagnostics::invalid_json(),
    }
}

fn field_status(value: Option<&serde_json::Value>) -> FinalMessageFieldStatus {
    match value {
        Some(serde_json::Value::Null) | None => FinalMessageFieldStatus::Missing,
        Some(_) => FinalMessageFieldStatus::Present,
    }
}

fn resolve_codex_binary() -> Result<PathBuf, CliError> {
    if let Some(path) = read_non_empty_env_path(ENV_CODEX_BIN)? {
        if path.is_file() {
            return Ok(path);
        }
        return Err(CliError::CodexCliUnavailable);
    }

    if let Some(path) = find_on_path("codex") {
        return Ok(path);
    }

    if let Some(path) = find_vscode_codex_binary() {
        return Ok(path);
    }

    Err(CliError::CodexCliUnavailable)
}

fn find_on_path(binary_name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(binary_name))
        .find(|candidate| candidate.is_file())
}

fn find_vscode_codex_binary() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let extension_roots = [
        home.join(".vscode/extensions"),
        home.join(".vscode-insiders/extensions"),
        home.join(".cursor/extensions"),
    ];

    let platform_dir = if cfg!(target_os = "linux") {
        "linux-x86_64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "macos-aarch64"
    } else if cfg!(target_os = "macos") {
        "macos-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        return None;
    };

    let executable = if cfg!(target_os = "windows") {
        "codex.exe"
    } else {
        "codex"
    };

    let mut candidates = Vec::new();
    for root in extension_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with("openai.chatgpt-") {
                let binary = path.join("bin").join(platform_dir).join(executable);
                if binary.is_file() {
                    candidates.push(binary);
                }
            }
        }
    }

    candidates.sort();
    candidates.pop()
}

enum ChildWaitOutcome {
    Completed {
        status: ExitStatus,
        elapsed_millis: u64,
    },
    TimedOut {
        elapsed_millis: u64,
    },
    WaitFailed {
        elapsed_millis: u64,
    },
}

trait WaitTimeout {
    fn wait_timeout(&mut self, timeout: Duration) -> ChildWaitOutcome;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, timeout: Duration) -> ChildWaitOutcome {
        let started_at = Instant::now();
        loop {
            match self.try_wait() {
                Ok(Some(status)) => {
                    return ChildWaitOutcome::Completed {
                        status,
                        elapsed_millis: elapsed_millis(started_at),
                    }
                }
                Ok(None) => {
                    if started_at.elapsed() >= timeout {
                        let _ = self.kill();
                        let _ = self.wait();
                        return ChildWaitOutcome::TimedOut {
                            elapsed_millis: elapsed_millis(started_at),
                        };
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => {
                    return ChildWaitOutcome::WaitFailed {
                        elapsed_millis: elapsed_millis(started_at),
                    }
                }
            }
        }
    }
}

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
