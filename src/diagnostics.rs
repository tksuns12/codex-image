use serde::Serialize;

use crate::config::ConfigError;
use crate::updater::UpdateError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    UsageOrConfig,
    Api,
    Filesystem,
    ResponseContract,
    Unknown,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        match self {
            Self::UsageOrConfig => 2,
            Self::Api => 4,
            Self::Filesystem => 5,
            Self::ResponseContract => 6,
            Self::Unknown => 1,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub error: ErrorDetails,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ErrorDetails {
    pub code: &'static str,
    pub message: &'static str,
    pub recoverable: bool,
    pub hint: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("configuration error")]
    Config(#[from] ConfigError),
    #[error("generated output write failed")]
    OutputWriteFailed,
    #[error("generated output verification failed")]
    OutputVerificationFailed,
    #[error("prompt file read failed")]
    PromptFileReadFailed,
    #[error("prompt file contained no prompts")]
    PromptFileEmpty,
    #[error("image generation response contract failure")]
    ImageGenerationResponseContract { source_message: String },
    #[error("Codex CLI is unavailable")]
    CodexCliUnavailable,
    #[error("Codex image generation failed")]
    CodexImageGenerationFailed { source_message: String },
    #[error("missing required skill install confirmation")]
    MissingInstallConfirmation,
    #[error("partial install target selection")]
    PartialInstallTargetSelection,
    #[error("no install targets selected in non-interactive mode")]
    NoInstallTargetsInNonInteractiveMode,
    #[error("interactive install target selection was cancelled")]
    InteractiveInstallSelectionCancelled,
    #[error("interactive install prompt failed")]
    InteractiveInstallPromptFailed,
    #[error("interactive install target selection was empty")]
    InteractiveInstallSelectionEmpty,
    #[error("missing required skill update confirmation")]
    MissingUpdateConfirmation,
    #[error("partial update target selection")]
    PartialUpdateTargetSelection,
    #[error("no update targets selected in non-interactive mode")]
    NoUpdateTargetsInNonInteractiveMode,
    #[error("interactive update target selection was cancelled")]
    InteractiveUpdateSelectionCancelled,
    #[error("interactive update prompt failed")]
    InteractiveUpdatePromptFailed,
    #[error("interactive update target selection was empty")]
    InteractiveUpdateSelectionEmpty,
    #[error("HOME is unavailable")]
    HomeUnavailable,
    #[error("current working directory is unavailable")]
    ProjectRootUnavailable,
    #[error("skill install write failed")]
    SkillInstallWriteFailed,
    #[error("skill install blocked by existing manual edit")]
    SkillInstallBlockedManualEdit,
    #[error("skill install delete failed")]
    SkillInstallDeleteFailed,
    #[error("skill install delete blocked by existing manual edit")]
    SkillInstallDeleteBlockedManualEdit,
    #[error("skill update write failed")]
    SkillUpdateWriteFailed,
    #[error("skill update blocked by existing manual edit")]
    SkillUpdateBlockedManualEdit,
    #[error("binary update failed")]
    BinaryUpdate(#[from] UpdateError),
    #[error("unexpected failure")]
    Unknown,
}

impl CliError {
    pub fn exit_code(&self) -> ExitCode {
        self.classification().exit_code
    }

    pub fn error_envelope(&self) -> ErrorEnvelope {
        let class = self.classification();
        ErrorEnvelope {
            error: ErrorDetails {
                code: class.code,
                message: class.message,
                recoverable: class.recoverable,
                hint: class.hint,
            },
        }
    }

    pub fn redacted_message(&self) -> &'static str {
        self.classification().message
    }

    fn classification(&self) -> ErrorClassification {
        match self {
            Self::Config(_) => ErrorClassification {
                code: "config.invalid",
                message: "configuration error",
                recoverable: true,
                hint: "Check CODEX_IMAGE_* configuration values.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::OutputWriteFailed | Self::OutputVerificationFailed => ErrorClassification {
                code: "filesystem.output_write_failed",
                message: "failed to write generated image output",
                recoverable: true,
                hint: "Ensure output paths are writable and retry.",
                exit_code: ExitCode::Filesystem,
            },
            Self::PromptFileReadFailed => ErrorClassification {
                code: "filesystem.prompt_file_read_failed",
                message: "failed to read prompt file",
                recoverable: true,
                hint: "Ensure --prompt-file points to a readable UTF-8 text file and retry.",
                exit_code: ExitCode::Filesystem,
            },
            Self::PromptFileEmpty => ErrorClassification {
                code: "usage.prompt_file_empty",
                message: "prompt file did not contain any prompts",
                recoverable: true,
                hint: "Add at least one non-empty, non-comment line to the prompt file and retry.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::ImageGenerationResponseContract { .. } => ErrorClassification {
                code: "response_contract.image_generation",
                message: "image generation response did not match expected schema",
                recoverable: false,
                hint: "Try again; if it persists, report the issue with request context.",
                exit_code: ExitCode::ResponseContract,
            },
            Self::CodexCliUnavailable => ErrorClassification {
                code: "config.codex_cli_unavailable",
                message: "Codex CLI executable was not found",
                recoverable: true,
                hint: "Install Codex or set CODEX_IMAGE_CODEX_BIN to the Codex executable.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::CodexImageGenerationFailed { .. } => ErrorClassification {
                code: "api.codex_image_generation_failed",
                message: "Codex image generation failed",
                recoverable: true,
                hint: "Retry generation in a moment, or verify Codex is installed and logged in.",
                exit_code: ExitCode::Api,
            },
            Self::MissingInstallConfirmation => ErrorClassification {
                code: "usage.install_confirmation_required",
                message: "skill install requires explicit confirmation",
                recoverable: true,
                hint: "Re-run with --yes to confirm non-interactive installation.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::PartialInstallTargetSelection => ErrorClassification {
                code: "usage.install_partial_target_selection",
                message: "skill install target selection is incomplete",
                recoverable: true,
                hint: "Provide at least one --tool and one --scope for non-interactive installation.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::NoInstallTargetsInNonInteractiveMode => ErrorClassification {
                code: "usage.install_no_targets_non_interactive",
                message: "no non-interactive install targets were selected",
                recoverable: true,
                hint: "Provide --tool and --scope flags with --yes, or run in an interactive terminal.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::InteractiveInstallSelectionCancelled => ErrorClassification {
                code: "usage.install_interactive_selection_cancelled",
                message: "interactive install selection was cancelled",
                recoverable: true,
                hint: "Re-run and confirm at least one target with Enter.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::InteractiveInstallPromptFailed => ErrorClassification {
                code: "usage.install_interactive_prompt_unavailable",
                message: "interactive install prompt is unavailable",
                recoverable: true,
                hint: "Use --tool/--scope with --yes when running non-interactively.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::InteractiveInstallSelectionEmpty => ErrorClassification {
                code: "usage.install_interactive_selection_empty",
                message: "interactive install selection was empty",
                recoverable: true,
                hint: "Select at least one target with Space before pressing Enter.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::MissingUpdateConfirmation => ErrorClassification {
                code: "usage.update_confirmation_required",
                message: "skill update requires explicit confirmation",
                recoverable: true,
                hint: "Re-run with --yes to confirm non-interactive update.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::PartialUpdateTargetSelection => ErrorClassification {
                code: "usage.update_partial_target_selection",
                message: "skill update target selection is incomplete",
                recoverable: true,
                hint: "Provide at least one --tool and one --scope for non-interactive updates.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::NoUpdateTargetsInNonInteractiveMode => ErrorClassification {
                code: "usage.update_no_targets_non_interactive",
                message: "no non-interactive update targets were selected",
                recoverable: true,
                hint: "Provide --tool and --scope flags with --yes, or run in an interactive terminal.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::InteractiveUpdateSelectionCancelled => ErrorClassification {
                code: "usage.update_interactive_selection_cancelled",
                message: "interactive update selection was cancelled",
                recoverable: true,
                hint: "Re-run and confirm at least one target with Enter.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::InteractiveUpdatePromptFailed => ErrorClassification {
                code: "usage.update_interactive_prompt_unavailable",
                message: "interactive update prompt is unavailable",
                recoverable: true,
                hint: "Use --tool/--scope with --yes when running non-interactively.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::InteractiveUpdateSelectionEmpty => ErrorClassification {
                code: "usage.update_interactive_selection_empty",
                message: "interactive update selection was empty",
                recoverable: true,
                hint: "Select at least one target with Space before pressing Enter.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::HomeUnavailable => ErrorClassification {
                code: "config.home_unavailable",
                message: "HOME is unavailable for global skill installation",
                recoverable: true,
                hint: "Set HOME to a writable directory and retry.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::ProjectRootUnavailable => ErrorClassification {
                code: "config.project_root_unavailable",
                message: "current directory is unavailable",
                recoverable: true,
                hint: "Run the command from an accessible project directory and retry.",
                exit_code: ExitCode::UsageOrConfig,
            },
            Self::SkillInstallWriteFailed => ErrorClassification {
                code: "filesystem.skill_install_write_failed",
                message: "failed to write managed SKILL.md",
                recoverable: true,
                hint: "Ensure target directories are writable and retry.",
                exit_code: ExitCode::Filesystem,
            },
            Self::SkillInstallBlockedManualEdit => ErrorClassification {
                code: "filesystem.skill_install_blocked_manual_edit",
                message: "existing SKILL.md is manual or tampered",
                recoverable: true,
                hint: "Re-run with --force to overwrite the existing file.",
                exit_code: ExitCode::Filesystem,
            },
            Self::SkillInstallDeleteFailed => ErrorClassification {
                code: "filesystem.skill_install_delete_failed",
                message: "failed to remove existing SKILL.md",
                recoverable: true,
                hint: "Ensure target directories are writable and retry, or use --force for protected content.",
                exit_code: ExitCode::Filesystem,
            },
            Self::SkillInstallDeleteBlockedManualEdit => ErrorClassification {
                code: "filesystem.skill_install_delete_blocked_manual_edit",
                message: "existing SKILL.md is manual or tampered",
                recoverable: true,
                hint: "Re-run with --force to remove the existing file.",
                exit_code: ExitCode::Filesystem,
            },
            Self::SkillUpdateWriteFailed => ErrorClassification {
                code: "filesystem.skill_update_write_failed",
                message: "failed to refresh managed SKILL.md",
                recoverable: true,
                hint: "Ensure target directories are writable and retry.",
                exit_code: ExitCode::Filesystem,
            },
            Self::SkillUpdateBlockedManualEdit => ErrorClassification {
                code: "filesystem.skill_update_blocked_manual_edit",
                message: "existing SKILL.md is manual or tampered",
                recoverable: true,
                hint: "Re-run with --force to overwrite the existing file.",
                exit_code: ExitCode::Filesystem,
            },
            Self::BinaryUpdate(source) => classify_binary_update_error(source),
            Self::Unknown => ErrorClassification {
                code: "unknown",
                message: "unexpected failure",
                recoverable: false,
                hint: "Re-run with supported commands or update the binary.",
                exit_code: ExitCode::Unknown,
            },
        }
    }
}

fn classify_binary_update_error(error: &UpdateError) -> ErrorClassification {
    match error {
        UpdateError::UnsupportedPlatform => ErrorClassification {
            code: "usage.update_unsupported_platform",
            message: "binary update is unavailable on this platform",
            recoverable: false,
            hint: "Use a supported release target: linux/x86_64, macos/x86_64, macos/aarch64, windows/x86_64.",
            exit_code: ExitCode::UsageOrConfig,
        },
        UpdateError::ConfirmationRequired => ErrorClassification {
            code: "usage.update_confirmation_required",
            message: "binary update requires --yes confirmation",
            recoverable: true,
            hint: "Re-run with --yes, or use --dry-run to validate without replacement.",
            exit_code: ExitCode::UsageOrConfig,
        },
        UpdateError::CurrentExecutableUnavailable => ErrorClassification {
            code: "config.update_current_executable_unavailable",
            message: "current executable path is unavailable",
            recoverable: true,
            hint: "Run from an installed binary path and retry.",
            exit_code: ExitCode::UsageOrConfig,
        },
        UpdateError::ReleaseLookupFailed => ErrorClassification {
            code: "api.update_release_lookup_failed",
            message: "failed to resolve release metadata",
            recoverable: true,
            hint: "Retry later or verify GitHub release availability.",
            exit_code: ExitCode::Api,
        },
        UpdateError::AssetDownloadFailed => ErrorClassification {
            code: "api.update_asset_download_failed",
            message: "failed to download release archive",
            recoverable: true,
            hint: "Retry later or verify network access to GitHub release assets.",
            exit_code: ExitCode::Api,
        },
        UpdateError::ReleaseMetadataInvalid
        | UpdateError::MissingReleaseAsset
        | UpdateError::DuplicateReleaseAsset
        | UpdateError::ArchiveInvalid
        | UpdateError::ArchivePathTraversal
        | UpdateError::ArchiveTopLevelDirectoryMismatch
        | UpdateError::ArchiveMissingRequiredFile
        | UpdateError::ArchiveDuplicateBinary => ErrorClassification {
            code: "response_contract.update_archive_invalid",
            message: "release archive did not match expected contract",
            recoverable: false,
            hint: "Retry with a different version; if it persists, report the release artifact issue.",
            exit_code: ExitCode::ResponseContract,
        },
        UpdateError::ReplacementUnsupported => ErrorClassification {
            code: "filesystem.update_replacement_unsupported",
            message: "binary replacement is unsupported on this platform",
            recoverable: true,
            hint: "Use --dry-run to validate the release archive, then replace the binary manually from the extracted artifact.",
            exit_code: ExitCode::Filesystem,
        },
        UpdateError::ReplacementFailed => ErrorClassification {
            code: "filesystem.update_replacement_failed",
            message: "failed to replace the current binary",
            recoverable: true,
            hint: "Ensure the current binary path is writable and retry.",
            exit_code: ExitCode::Filesystem,
        },
    }
}

#[derive(Debug, Clone, Copy)]
struct ErrorClassification {
    code: &'static str,
    message: &'static str,
    recoverable: bool,
    hint: &'static str,
    exit_code: ExitCode,
}
