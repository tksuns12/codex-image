use std::fs;

use codex_image::config::ConfigError;
use codex_image::diagnostics::{CliError, ExitCode};
use codex_image::generation_diagnostics::{
    write_generation_diagnostics, BatchDiagnosticsSummary, CodexRunDiagnostics, CodexRunStatus,
    FinalMessageDiagnostics, FinalMessageFieldStatus, GenerationDiagnostics,
    GenerationDiagnosticsMetadata, GenerationDiagnosticsMode, GenerationDiagnosticsResult,
    GenerationDiagnosticsStdoutMode, PromptSourceKind, SanitizedCommand,
    GENERATION_DIAGNOSTICS_SCHEMA, GENERATION_DIAGNOSTICS_SCHEMA_VERSION,
};
use codex_image::updater::UpdateError;
use tempfile::TempDir;

fn parse_envelope(err: &CliError) -> serde_json::Value {
    serde_json::to_value(err.error_envelope()).expect("error envelope serializes")
}

fn assert_error_contract_shape(json: &serde_json::Value) {
    let root = json
        .as_object()
        .expect("error envelope root should be an object");
    assert_eq!(
        root.len(),
        1,
        "error envelope root must only contain `error`"
    );

    let error = root
        .get("error")
        .and_then(serde_json::Value::as_object)
        .expect("error envelope `error` field should be an object");
    assert_eq!(
        error.len(),
        4,
        "error object must only contain code/message/recoverable/hint"
    );

    assert!(error
        .get("code")
        .and_then(serde_json::Value::as_str)
        .is_some());
    assert!(error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .is_some());
    assert!(error
        .get("recoverable")
        .and_then(serde_json::Value::as_bool)
        .is_some());
    assert!(error
        .get("hint")
        .and_then(serde_json::Value::as_str)
        .is_some());
}

fn sample_generation_diagnostics() -> GenerationDiagnostics {
    let leaked_source = CliError::CodexImageGenerationFailed {
        source_message: "Bearer sk-test b64_json /tmp/codex-image-secret /home/alice CODEX_IMAGE_CODEX_BIN raw final-message JSON image_path note".to_string(),
    };

    GenerationDiagnostics::new(GenerationDiagnosticsMetadata::for_invocation_at(
        1_700_000_000,
        GenerationDiagnosticsMode::Batch,
        GenerationDiagnosticsResult::Failure,
        PromptSourceKind::PromptFile,
        GenerationDiagnosticsStdoutMode::Json,
        300,
    ))
    .with_batch_summary(BatchDiagnosticsSummary {
        planned_items: 2,
        completed_items: 1,
        failed_item_index: Some(2),
    })
    .with_failure(&leaked_source)
    .with_runs(vec![CodexRunDiagnostics::failure(
        1,
        Some(1),
        300,
        42,
        Some(1),
        CodexRunStatus::FinalMessageInvalid,
        FinalMessageDiagnostics::contract_invalid(
            FinalMessageFieldStatus::Present,
            FinalMessageFieldStatus::Present,
        ),
        &leaked_source,
    )])
}

#[test]
fn diagnostics_debug_diagnostics_schema_v1_serializes_only_sanitized_fields() {
    let diagnostics = sample_generation_diagnostics();
    let rendered = serde_json::to_string_pretty(&diagnostics).expect("diagnostics serialize");
    let json: serde_json::Value = serde_json::from_str(&rendered).expect("diagnostics json");

    assert_eq!(json["schema"], GENERATION_DIAGNOSTICS_SCHEMA);
    assert_eq!(
        json["schema_version"],
        GENERATION_DIAGNOSTICS_SCHEMA_VERSION
    );
    assert_eq!(json["metadata"]["mode"], "batch");
    assert_eq!(json["metadata"]["result"], "failure");
    assert_eq!(json["metadata"]["prompt_source"], "prompt_file");
    assert_eq!(json["metadata"]["stdout_mode"], "json");
    assert_eq!(json["metadata"]["command"]["program"], "codex");
    assert_eq!(
        json["metadata"]["command"],
        serde_json::to_value(SanitizedCommand::codex_subprocess()).unwrap()
    );
    assert_eq!(json["batch"]["planned_items"], 2);
    assert_eq!(json["batch"]["completed_items"], 1);
    assert_eq!(json["batch"]["failed_item_index"], 2);
    assert_eq!(json["failure"]["code"], "api.codex_image_generation_failed");
    assert_eq!(json["runs"][0]["phase"], "generate_image");
    assert_eq!(json["runs"][0]["outcome"], "failed");
    assert_eq!(json["runs"][0]["status"], "final_message_invalid");
    assert_eq!(
        json["runs"][0]["final_message"]["status"],
        "contract_invalid"
    );
    assert_eq!(json["redaction"]["prompt_text"], "omitted");

    for forbidden in [
        "Bearer",
        "sk-test",
        "b64_json",
        "/tmp/codex-image-secret",
        "/home/alice",
        "CODEX_IMAGE_CODEX_BIN",
        "raw final-message JSON",
        "\"image_path\"",
        "\"note\"",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "diagnostics leaked forbidden sentinel: {forbidden}\n{rendered}"
        );
    }
}

#[test]
fn diagnostics_debug_diagnostics_writer_creates_parent_and_maps_failures_to_output_write() {
    let temp = TempDir::new().expect("tempdir");
    let diagnostics = sample_generation_diagnostics();
    let diagnostics_path = temp.path().join("nested").join("diagnostics.json");

    write_generation_diagnostics(&diagnostics_path, &diagnostics).expect("diagnostics write");

    let rendered = fs::read_to_string(&diagnostics_path).expect("diagnostics file exists");
    let json: serde_json::Value = serde_json::from_str(&rendered).expect("diagnostics json");
    assert_eq!(json["schema_version"], 1);

    let parent_file = temp.path().join("not-a-directory");
    fs::write(&parent_file, "not a directory").expect("write parent sentinel");
    let err = write_generation_diagnostics(&parent_file.join("diagnostics.json"), &diagnostics)
        .expect_err("file parent should fail");
    let envelope = parse_envelope(&err);

    assert_eq!(err.exit_code(), ExitCode::Filesystem);
    assert_eq!(envelope["error"]["code"], "filesystem.output_write_failed");
}

#[test]
fn diagnostics_config_error_maps_to_usage_config_exit_and_shape() {
    let err = CliError::Config(ConfigError::InvalidValue {
        key: "CODEX_IMAGE_CODEX_BIN",
    });

    assert_eq!(err.exit_code(), ExitCode::UsageOrConfig);

    let json = parse_envelope(&err);
    assert_eq!(json["error"]["code"], "config.invalid");
    assert_eq!(json["error"]["message"], "configuration error");
    assert_eq!(json["error"]["recoverable"], true);
    assert_eq!(
        json["error"]["hint"],
        "Check CODEX_IMAGE_* configuration values."
    );

    let rendered = serde_json::to_string(&json).unwrap();
    assert!(!rendered.contains("CODEX_IMAGE_CODEX_BIN"));
}

#[test]
fn diagnostics_output_write_or_verify_failures_map_to_filesystem_exit() {
    let write_err = CliError::OutputWriteFailed;
    let write_json = parse_envelope(&write_err);
    assert_eq!(write_err.exit_code(), ExitCode::Filesystem);
    assert_eq!(
        write_json["error"]["code"],
        "filesystem.output_write_failed"
    );
    assert_eq!(
        write_json["error"]["message"],
        "failed to write generated image output"
    );

    let verify_err = CliError::OutputVerificationFailed;
    let verify_json = parse_envelope(&verify_err);
    assert_eq!(verify_err.exit_code(), ExitCode::Filesystem);
    assert_eq!(
        verify_json["error"]["code"],
        "filesystem.output_write_failed"
    );
}

#[test]
fn diagnostics_image_response_contract_maps_to_response_contract_exit() {
    let err = CliError::ImageGenerationResponseContract {
        source_message: "unexpected generated image path".to_string(),
    };

    assert_eq!(err.exit_code(), ExitCode::ResponseContract);

    let json = parse_envelope(&err);
    assert_eq!(json["error"]["code"], "response_contract.image_generation");
    assert_eq!(
        json["error"]["message"],
        "image generation response did not match expected schema"
    );
    assert_eq!(json["error"]["recoverable"], false);
    assert_eq!(
        json["error"]["hint"],
        "Try again; if it persists, report the issue with request context."
    );

    let rendered = serde_json::to_string(&json).unwrap();
    assert!(!rendered.contains("unexpected generated image path"));
}

#[test]
fn diagnostics_codex_backend_errors_are_redacted_and_actionable() {
    let unavailable = CliError::CodexCliUnavailable;
    assert_eq!(unavailable.exit_code(), ExitCode::UsageOrConfig);
    let unavailable_json = parse_envelope(&unavailable);
    assert_eq!(
        unavailable_json["error"]["code"],
        "config.codex_cli_unavailable"
    );
    assert_eq!(
        unavailable_json["error"]["hint"],
        "Install Codex or set CODEX_IMAGE_CODEX_BIN to the Codex executable."
    );

    let failed = CliError::CodexImageGenerationFailed {
        source_message: "Bearer sk-secret raw codex output".to_string(),
    };
    assert_eq!(failed.exit_code(), ExitCode::Api);
    let failed_json = parse_envelope(&failed);
    assert_eq!(
        failed_json["error"]["code"],
        "api.codex_image_generation_failed"
    );

    let rendered = serde_json::to_string(&failed_json).unwrap();
    assert!(!rendered.contains("Bearer"));
    assert!(!rendered.contains("sk-secret"));
    assert!(!rendered.contains("raw codex output"));
}

#[test]
fn diagnostics_skill_install_confirmation_target_selection_home_and_write_failures_are_redacted() {
    let missing_yes = CliError::MissingInstallConfirmation;
    assert_eq!(missing_yes.exit_code(), ExitCode::UsageOrConfig);
    let missing_yes_json = parse_envelope(&missing_yes);
    assert_eq!(
        missing_yes_json["error"]["code"],
        "usage.install_confirmation_required"
    );

    let partial_targets = CliError::PartialInstallTargetSelection;
    assert_eq!(partial_targets.exit_code(), ExitCode::UsageOrConfig);
    let partial_targets_json = parse_envelope(&partial_targets);
    assert_eq!(
        partial_targets_json["error"]["code"],
        "usage.install_partial_target_selection"
    );

    let no_targets = CliError::NoInstallTargetsInNonInteractiveMode;
    assert_eq!(no_targets.exit_code(), ExitCode::UsageOrConfig);
    let no_targets_json = parse_envelope(&no_targets);
    assert_eq!(
        no_targets_json["error"]["code"],
        "usage.install_no_targets_non_interactive"
    );

    let selection_cancelled = CliError::InteractiveInstallSelectionCancelled;
    assert_eq!(selection_cancelled.exit_code(), ExitCode::UsageOrConfig);
    let selection_cancelled_json = parse_envelope(&selection_cancelled);
    assert_eq!(
        selection_cancelled_json["error"]["code"],
        "usage.install_interactive_selection_cancelled"
    );

    let prompt_failed = CliError::InteractiveInstallPromptFailed;
    assert_eq!(prompt_failed.exit_code(), ExitCode::UsageOrConfig);
    let prompt_failed_json = parse_envelope(&prompt_failed);
    assert_eq!(
        prompt_failed_json["error"]["code"],
        "usage.install_interactive_prompt_unavailable"
    );

    let selection_empty = CliError::InteractiveInstallSelectionEmpty;
    assert_eq!(selection_empty.exit_code(), ExitCode::UsageOrConfig);
    let selection_empty_json = parse_envelope(&selection_empty);
    assert_eq!(
        selection_empty_json["error"]["code"],
        "usage.install_interactive_selection_empty"
    );

    let home_missing = CliError::HomeUnavailable;
    assert_eq!(home_missing.exit_code(), ExitCode::UsageOrConfig);
    let home_missing_json = parse_envelope(&home_missing);
    assert_eq!(
        home_missing_json["error"]["code"],
        "config.home_unavailable"
    );

    let write_failed = CliError::SkillInstallWriteFailed;
    assert_eq!(write_failed.exit_code(), ExitCode::Filesystem);
    let write_failed_json = parse_envelope(&write_failed);
    assert_eq!(
        write_failed_json["error"]["code"],
        "filesystem.skill_install_write_failed"
    );

    let blocked = CliError::SkillInstallBlockedManualEdit;
    assert_eq!(blocked.exit_code(), ExitCode::Filesystem);
    let blocked_json = parse_envelope(&blocked);
    assert_eq!(
        blocked_json["error"]["code"],
        "filesystem.skill_install_blocked_manual_edit"
    );

    let delete_failed = CliError::SkillInstallDeleteFailed;
    assert_eq!(delete_failed.exit_code(), ExitCode::Filesystem);
    let delete_failed_json = parse_envelope(&delete_failed);
    assert_eq!(
        delete_failed_json["error"]["code"],
        "filesystem.skill_install_delete_failed"
    );

    let delete_blocked = CliError::SkillInstallDeleteBlockedManualEdit;
    assert_eq!(delete_blocked.exit_code(), ExitCode::Filesystem);
    let delete_blocked_json = parse_envelope(&delete_blocked);
    assert_eq!(
        delete_blocked_json["error"]["code"],
        "filesystem.skill_install_delete_blocked_manual_edit"
    );

    let rendered = serde_json::to_string(&delete_blocked_json).unwrap();
    assert!(!rendered.contains("/tmp/"));
    assert!(!rendered.contains("HOME"));
    assert!(!rendered.contains("Bearer"));
}

#[test]
fn diagnostics_skill_update_confirmation_target_selection_and_filesystem_failures_are_redacted() {
    let missing_yes = CliError::MissingUpdateConfirmation;
    assert_eq!(missing_yes.exit_code(), ExitCode::UsageOrConfig);
    let missing_yes_json = parse_envelope(&missing_yes);
    assert_eq!(
        missing_yes_json["error"]["code"],
        "usage.update_confirmation_required"
    );
    assert_eq!(
        missing_yes_json["error"]["hint"],
        "Re-run with --yes to confirm non-interactive update."
    );

    let partial_targets = CliError::PartialUpdateTargetSelection;
    assert_eq!(partial_targets.exit_code(), ExitCode::UsageOrConfig);
    let partial_targets_json = parse_envelope(&partial_targets);
    assert_eq!(
        partial_targets_json["error"]["code"],
        "usage.update_partial_target_selection"
    );

    let no_targets = CliError::NoUpdateTargetsInNonInteractiveMode;
    assert_eq!(no_targets.exit_code(), ExitCode::UsageOrConfig);
    let no_targets_json = parse_envelope(&no_targets);
    assert_eq!(
        no_targets_json["error"]["code"],
        "usage.update_no_targets_non_interactive"
    );

    let selection_cancelled = CliError::InteractiveUpdateSelectionCancelled;
    assert_eq!(selection_cancelled.exit_code(), ExitCode::UsageOrConfig);
    let selection_cancelled_json = parse_envelope(&selection_cancelled);
    assert_eq!(
        selection_cancelled_json["error"]["code"],
        "usage.update_interactive_selection_cancelled"
    );

    let prompt_failed = CliError::InteractiveUpdatePromptFailed;
    assert_eq!(prompt_failed.exit_code(), ExitCode::UsageOrConfig);
    let prompt_failed_json = parse_envelope(&prompt_failed);
    assert_eq!(
        prompt_failed_json["error"]["code"],
        "usage.update_interactive_prompt_unavailable"
    );

    let selection_empty = CliError::InteractiveUpdateSelectionEmpty;
    assert_eq!(selection_empty.exit_code(), ExitCode::UsageOrConfig);
    let selection_empty_json = parse_envelope(&selection_empty);
    assert_eq!(
        selection_empty_json["error"]["code"],
        "usage.update_interactive_selection_empty"
    );

    let write_failed = CliError::SkillUpdateWriteFailed;
    assert_eq!(write_failed.exit_code(), ExitCode::Filesystem);
    let write_failed_json = parse_envelope(&write_failed);
    assert_eq!(
        write_failed_json["error"]["code"],
        "filesystem.skill_update_write_failed"
    );

    let blocked = CliError::SkillUpdateBlockedManualEdit;
    assert_eq!(blocked.exit_code(), ExitCode::Filesystem);
    let blocked_json = parse_envelope(&blocked);
    assert_eq!(
        blocked_json["error"]["code"],
        "filesystem.skill_update_blocked_manual_edit"
    );

    let rendered = serde_json::to_string(&blocked_json).unwrap();
    assert!(!rendered.contains("/tmp/"));
    assert!(!rendered.contains("HOME"));
    assert!(!rendered.contains("Bearer"));
    assert!(!rendered.contains("sk-secret"));
}

#[test]
fn diagnostics_binary_update_errors_map_to_stable_codes_without_leaks() {
    let cases = [
        (
            UpdateError::UnsupportedPlatform,
            ExitCode::UsageOrConfig,
            "usage.update_unsupported_platform",
        ),
        (
            UpdateError::ConfirmationRequired,
            ExitCode::UsageOrConfig,
            "usage.update_confirmation_required",
        ),
        (
            UpdateError::CurrentExecutableUnavailable,
            ExitCode::UsageOrConfig,
            "config.update_current_executable_unavailable",
        ),
        (
            UpdateError::ReleaseLookupFailed,
            ExitCode::Api,
            "api.update_release_lookup_failed",
        ),
        (
            UpdateError::AssetDownloadFailed,
            ExitCode::Api,
            "api.update_asset_download_failed",
        ),
        (
            UpdateError::ReleaseMetadataInvalid,
            ExitCode::ResponseContract,
            "response_contract.update_archive_invalid",
        ),
        (
            UpdateError::MissingReleaseAsset,
            ExitCode::ResponseContract,
            "response_contract.update_archive_invalid",
        ),
        (
            UpdateError::ArchiveInvalid,
            ExitCode::ResponseContract,
            "response_contract.update_archive_invalid",
        ),
        (
            UpdateError::ReplacementUnsupported,
            ExitCode::Filesystem,
            "filesystem.update_replacement_unsupported",
        ),
        (
            UpdateError::ReplacementFailed,
            ExitCode::Filesystem,
            "filesystem.update_replacement_failed",
        ),
    ];

    for (source, expected_exit, expected_code) in cases {
        let err = CliError::BinaryUpdate(source);
        assert_eq!(err.exit_code(), expected_exit);

        let json = parse_envelope(&err);
        assert_eq!(json["error"]["code"], expected_code);

        let rendered = serde_json::to_string(&json).expect("json serializes");
        assert!(!rendered.contains("https://"));
        assert!(!rendered.contains("/tmp/"));
        assert!(!rendered.contains("Bearer"));
        assert!(!rendered.contains("HOME"));
    }
}

#[test]
fn diagnostics_windows_replacement_policy_error_is_redacted_and_actionable() {
    let err = CliError::BinaryUpdate(UpdateError::ReplacementUnsupported);

    assert_eq!(err.exit_code(), ExitCode::Filesystem);

    let json = parse_envelope(&err);
    assert_eq!(
        json["error"]["code"],
        "filesystem.update_replacement_unsupported"
    );
    assert_eq!(
        json["error"]["message"],
        "binary replacement is unsupported on this platform"
    );
    assert_eq!(json["error"]["recoverable"], true);
    assert_eq!(
        json["error"]["hint"],
        "Use --dry-run to validate the release archive, then replace the binary manually from the extracted artifact."
    );

    let rendered = serde_json::to_string(&json).expect("json serializes");
    assert!(!rendered.contains("https://"));
    assert!(!rendered.contains("/tmp/"));
    assert!(!rendered.contains("HOME"));
}

#[test]
fn diagnostics_unknown_fallback_is_stable() {
    let err = CliError::Unknown;

    assert_eq!(err.exit_code(), ExitCode::Unknown);

    let json = parse_envelope(&err);
    assert_eq!(json["error"]["code"], "unknown");
    assert_eq!(json["error"]["message"], "unexpected failure");
    assert_eq!(json["error"]["recoverable"], false);
    assert_eq!(
        json["error"]["hint"],
        "Re-run with supported commands or update the binary."
    );
}

#[test]
fn diagnostics_exit_code_taxonomy_is_stable() {
    assert_eq!(ExitCode::Unknown.as_i32(), 1);
    assert_eq!(ExitCode::UsageOrConfig.as_i32(), 2);
    assert_eq!(ExitCode::Api.as_i32(), 4);
    assert_eq!(ExitCode::Filesystem.as_i32(), 5);
    assert_eq!(ExitCode::ResponseContract.as_i32(), 6);
}

#[test]
fn diagnostics_all_error_envelopes_keep_exact_machine_readable_shape() {
    let cases = [
        CliError::Config(ConfigError::InvalidValue {
            key: "CODEX_IMAGE_CODEX_BIN",
        }),
        CliError::OutputWriteFailed,
        CliError::OutputVerificationFailed,
        CliError::ImageGenerationResponseContract {
            source_message: "generated path".to_string(),
        },
        CliError::CodexCliUnavailable,
        CliError::CodexImageGenerationFailed {
            source_message: "codex failed".to_string(),
        },
        CliError::MissingInstallConfirmation,
        CliError::PartialInstallTargetSelection,
        CliError::NoInstallTargetsInNonInteractiveMode,
        CliError::InteractiveInstallSelectionCancelled,
        CliError::InteractiveInstallPromptFailed,
        CliError::InteractiveInstallSelectionEmpty,
        CliError::MissingUpdateConfirmation,
        CliError::PartialUpdateTargetSelection,
        CliError::NoUpdateTargetsInNonInteractiveMode,
        CliError::InteractiveUpdateSelectionCancelled,
        CliError::InteractiveUpdatePromptFailed,
        CliError::InteractiveUpdateSelectionEmpty,
        CliError::HomeUnavailable,
        CliError::ProjectRootUnavailable,
        CliError::SkillInstallWriteFailed,
        CliError::SkillInstallBlockedManualEdit,
        CliError::SkillInstallDeleteFailed,
        CliError::SkillInstallDeleteBlockedManualEdit,
        CliError::SkillUpdateWriteFailed,
        CliError::SkillUpdateBlockedManualEdit,
        CliError::BinaryUpdate(UpdateError::ConfirmationRequired),
        CliError::BinaryUpdate(UpdateError::UnsupportedPlatform),
        CliError::BinaryUpdate(UpdateError::ReleaseLookupFailed),
        CliError::BinaryUpdate(UpdateError::AssetDownloadFailed),
        CliError::BinaryUpdate(UpdateError::ArchiveInvalid),
        CliError::BinaryUpdate(UpdateError::ReplacementUnsupported),
        CliError::BinaryUpdate(UpdateError::ReplacementFailed),
        CliError::Unknown,
    ];

    for err in cases {
        let json = parse_envelope(&err);
        assert_error_contract_shape(&json);
    }
}
