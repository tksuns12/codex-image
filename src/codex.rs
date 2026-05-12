use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::config::{read_non_empty_env_path, ENV_CODEX_BIN};
use crate::diagnostics::CliError;

pub const DEFAULT_CODEX_EXEC_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_CODEX_EXEC_TIMEOUT: Duration =
    Duration::from_secs(DEFAULT_CODEX_EXEC_TIMEOUT_SECS);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexGenerationOptions {
    pub timeout: Duration,
}

impl Default for CodexGenerationOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_CODEX_EXEC_TIMEOUT,
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

    let codex_bin = resolve_codex_binary()?;
    let last_message_path = out_dir.join(format!(
        ".codex-image-last-message-{}.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&last_message_path);

    let codex_prompt = build_codex_prompt(prompt);
    let source_not_before = current_filesystem_timestamp_floor();
    let mut child = Command::new(&codex_bin)
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
        .map_err(|_| CliError::CodexImageGenerationFailed {
            source_message: "failed to spawn Codex CLI".to_string(),
        })?;

    let status = child.wait_timeout(options.timeout).map_err(|err| {
        let _ = fs::remove_file(&last_message_path);
        err
    })?;

    if !status.success() {
        let _ = fs::remove_file(&last_message_path);
        return Err(CliError::CodexImageGenerationFailed {
            source_message: format!("Codex CLI exited with status {status}"),
        });
    }

    let final_message = fs::read_to_string(&last_message_path).map_err(|_| {
        let _ = fs::remove_file(&last_message_path);
        CliError::ImageGenerationResponseContract {
            source_message: "Codex CLI did not write final image JSON".to_string(),
        }
    })?;
    let _ = fs::remove_file(&last_message_path);

    let parsed = parse_final_message(&final_message)?;
    let source_path = PathBuf::from(parsed.image_path);
    if !source_path.is_file() {
        return Err(CliError::ImageGenerationResponseContract {
            source_message: "Codex image path does not exist".to_string(),
        });
    }

    Ok(CodexImageGeneration {
        source_path,
        source_not_before,
        note: parsed.note,
    })
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

fn parse_final_message(message: &str) -> Result<CodexFinalMessage, CliError> {
    if let Ok(parsed) = serde_json::from_str::<CodexFinalMessage>(message.trim()) {
        return Ok(parsed);
    }

    let start = message
        .find('{')
        .ok_or_else(|| CliError::ImageGenerationResponseContract {
            source_message: "Codex final message did not contain JSON".to_string(),
        })?;
    let end = message
        .rfind('}')
        .ok_or_else(|| CliError::ImageGenerationResponseContract {
            source_message: "Codex final message did not contain complete JSON".to_string(),
        })?;

    serde_json::from_str::<CodexFinalMessage>(&message[start..=end]).map_err(|_| {
        CliError::ImageGenerationResponseContract {
            source_message: "Codex final image JSON did not match expected schema".to_string(),
        }
    })
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

trait WaitTimeout {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<std::process::ExitStatus, CliError>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Result<std::process::ExitStatus, CliError> {
        let started_at = Instant::now();
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {
                    if started_at.elapsed() >= timeout {
                        let _ = self.kill();
                        let _ = self.wait();
                        return Err(CliError::CodexImageGenerationFailed {
                            source_message: "Codex CLI image generation timed out".to_string(),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => {
                    return Err(CliError::CodexImageGenerationFailed {
                        source_message: "failed to wait for Codex CLI".to_string(),
                    })
                }
            }
        }
    }
}
