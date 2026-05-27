use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

fn write_fake_codex(temp: &TempDir, source_image: &std::path::Path) -> std::path::PathBuf {
    let script_path = temp.path().join("fake-codex");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
last_message=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-last-message)
      shift
      last_message="$1"
      ;;
  esac
  shift || true
done
if [ -z "$last_message" ]; then
  exit 41
fi
touch "{}"
printf '{{"image_path":"{}","note":"fake codex generated image"}}' > "$last_message"
"#,
        source_image.display(),
        source_image.display()
    );
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    script_path
}

fn write_failing_codex(temp: &TempDir) -> std::path::PathBuf {
    let script_path = temp.path().join("failing-codex");
    fs::write(
        &script_path,
        "#!/usr/bin/env bash\necho 'Bearer secret should not leak' >&2\nexit 42\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    script_path
}

fn write_missing_final_message_codex(temp: &TempDir) -> std::path::PathBuf {
    let script_path = temp.path().join("missing-final-message-codex");
    fs::write(
        &script_path,
        "#!/usr/bin/env bash\necho 'sk-missing-final-message should not leak' >&2\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    script_path
}

fn write_malformed_final_message_codex(temp: &TempDir) -> std::path::PathBuf {
    let script_path = temp.path().join("malformed-final-message-codex");
    let script = r#"#!/usr/bin/env bash
set -eu
last_message=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-last-message)
      shift
      last_message="$1"
      ;;
  esac
  shift || true
done
if [ -z "$last_message" ]; then
  exit 41
fi
printf '} Bearer sk-malformed-final-message b64_json {broken' > "$last_message"
"#;
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    script_path
}

fn write_untrusted_path_codex(
    temp: &TempDir,
    source_image: &std::path::Path,
) -> std::path::PathBuf {
    let script_path = temp.path().join("untrusted-path-codex");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
last_message=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-last-message)
      shift
      last_message="$1"
      ;;
  esac
  shift || true
done
if [ -z "$last_message" ]; then
  exit 41
fi
printf '{{"image_path":"{}","note":"Bearer stale source should not leak"}}' > "$last_message"
"#,
        source_image.display()
    );
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    script_path
}

fn wait_until_file_predates_next_invocation(path: &std::path::Path) {
    let modified = fs::metadata(path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    while SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        <= modified
    {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn write_sleeping_codex(
    temp: &TempDir,
    sleep_secs: u64,
    token_fixture: &str,
) -> std::path::PathBuf {
    let script_path = temp.path().join("sleeping-codex");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
last_message=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-last-message)
      shift
      last_message="$1"
      ;;
  esac
  shift || true
done
if [ -z "$last_message" ]; then
  exit 41
fi
printf '{{"image_path":"ignore-timeout.png","note":"Bearer secret {}"}}' > "$last_message"
sleep {}
"#,
        token_fixture, sleep_secs
    );
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    script_path
}

fn write_arg_recording_codex(
    temp: &TempDir,
    source_image: &std::path::Path,
    argv_log: &std::path::Path,
) -> std::path::PathBuf {
    let script_path = temp.path().join("arg-recording-codex");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
last_message=""
: > "{argv_log}"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "{argv_log}"
done
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-last-message)
      shift
      last_message="$1"
      ;;
  esac
  shift || true
done
if [ -z "$last_message" ]; then
  exit 41
fi
touch "{source_image}"
printf '{{"image_path":"{source_image}","note":"fake codex generated image"}}' > "$last_message"
"#,
        argv_log = argv_log.display(),
        source_image = source_image.display(),
    );
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    script_path
}

fn write_batch_arg_recording_codex(
    temp: &TempDir,
    first_source_image: &std::path::Path,
    second_source_image: &std::path::Path,
    argv_log: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let script_path = temp.path().join("batch-arg-recording-codex");
    let counter_file = temp.path().join("batch-codex-counter");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
last_message=""
counter_file="{counter_file}"
if [ ! -f "$counter_file" ]; then
  printf '0' > "$counter_file"
fi
call_count=$(cat "$counter_file")
call_count=$((call_count + 1))
printf '%s' "$call_count" > "$counter_file"

printf 'CALL %s\n' "$call_count" >> "{argv_log}"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "{argv_log}"
done
printf -- '---\n' >> "{argv_log}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-last-message)
      shift
      last_message="$1"
      ;;
  esac
  shift || true
done
if [ -z "$last_message" ]; then
  exit 41
fi

case "$call_count" in
  1)
    image_path="{first_source_image}"
    ;;
  2)
    image_path="{second_source_image}"
    ;;
  *)
    exit 43
    ;;
esac

touch "$image_path"
printf '{{"image_path":"%s","note":"fake codex generated image"}}' "$image_path" > "$last_message"
"#,
        counter_file = counter_file.display(),
        argv_log = argv_log.display(),
        first_source_image = first_source_image.display(),
        second_source_image = second_source_image.display(),
    );
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    (script_path, counter_file)
}

fn write_batch_second_fail_codex(
    temp: &TempDir,
    first_source_image: &std::path::Path,
    argv_log: &std::path::Path,
    stderr_sentinel: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let script_path = temp.path().join("batch-second-fail-codex");
    let counter_file = temp.path().join("batch-second-fail-counter");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
last_message=""
counter_file="{counter_file}"
if [ ! -f "$counter_file" ]; then
  printf '0' > "$counter_file"
fi
call_count=$(cat "$counter_file")
call_count=$((call_count + 1))
printf '%s' "$call_count" > "$counter_file"

printf 'CALL %s\n' "$call_count" >> "{argv_log}"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "{argv_log}"
done
printf -- '---\n' >> "{argv_log}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --output-last-message)
      shift
      last_message="$1"
      ;;
  esac
  shift || true
done
if [ -z "$last_message" ]; then
  exit 41
fi

if [ "$call_count" -eq 2 ]; then
  printf 'Bearer token %s\n' "{stderr_sentinel}" >&2
  printf 'prompt-second: second prompt secret\n' >&2
  printf 'path-second: /tmp/sensitive/path.txt\n' >&2
  exit 42
fi

if [ "$call_count" -gt 2 ]; then
  exit 43
fi

touch "{first_source_image}"
printf '{{"image_path":"{first_source_image}","note":"fake codex generated image"}}' > "$last_message"
"#,
        counter_file = counter_file.display(),
        argv_log = argv_log.display(),
        first_source_image = first_source_image.display(),
        stderr_sentinel = stderr_sentinel,
    );
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    (script_path, counter_file)
}

fn count_last_message_artifacts(out_dir: &std::path::Path) -> usize {
    fs::read_dir(out_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".codex-image-last-message-")
        })
        .count()
}

fn assert_debug_diagnostics_omits(rendered: &str, forbidden: &[&str]) {
    for sentinel in forbidden {
        assert!(
            !rendered.contains(sentinel),
            "debug diagnostics leaked forbidden sentinel: {sentinel}\n{rendered}"
        );
    }
}

#[test]
fn generate_debug_diagnostics_single_success_writes_sanitized_sidecar_without_stdout_changes() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let out_dir = temp.path().join("images");
    let diagnostics_path = temp.path().join("nested").join("diagnostics.json");
    let prompt = "red circle Bearer sk-diagnostics-success b64_json";

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg(prompt)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--timeout")
        .arg("7")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stdout_manifest: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(stdout_manifest["prompt"], prompt);
    assert!(out_dir.join("manifest.json").is_file());

    let rendered = fs::read_to_string(&diagnostics_path).unwrap();
    let diagnostics: Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(diagnostics["schema"], "codex-image.generation-diagnostics");
    assert_eq!(diagnostics["schema_version"], 1);
    assert_eq!(diagnostics["metadata"]["mode"], "single");
    assert_eq!(diagnostics["metadata"]["result"], "success");
    assert_eq!(
        diagnostics["metadata"]["prompt_source"],
        "positional_prompt"
    );
    assert_eq!(diagnostics["metadata"]["stdout_mode"], "json");
    assert_eq!(diagnostics["metadata"]["timeout_seconds"], 7);
    assert!(diagnostics.get("failure").is_none());

    let runs = diagnostics["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run["item_index"], Value::Null);
    assert_eq!(run["phase"], "generate_image");
    assert_eq!(run["outcome"], "succeeded");
    assert_eq!(run["status"], "succeeded");
    assert_eq!(run["timeout_seconds"], 7);
    assert_eq!(run["timed_out"], false);
    assert_eq!(run["exit_code"], 0);
    assert_eq!(run["command"]["program"], "codex");
    assert_eq!(run["final_message"]["status"], "parsed");
    assert_eq!(run["final_message"]["presence"], "present");
    assert_eq!(run["final_message"]["parse"], "parsed");
    assert_eq!(run["final_message"]["image_path_status"], "present");
    assert_eq!(run["final_message"]["note_status"], "present");

    let temp_path = temp.path().to_string_lossy().to_string();
    let out_path = out_dir.to_string_lossy().to_string();
    let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
    let fake_codex_path = fake_codex.to_string_lossy().to_string();
    let source_path = source_image.to_string_lossy().to_string();
    assert_debug_diagnostics_omits(
        &rendered,
        &[
            prompt,
            "Bearer",
            "sk-diagnostics-success",
            "b64_json",
            "fake codex generated image",
            "CODEX_IMAGE_CODEX_BIN",
            temp_path.as_str(),
            out_path.as_str(),
            diagnostics_file_path.as_str(),
            fake_codex_path.as_str(),
            source_path.as_str(),
            ".codex-image-last-message-",
            "image_path\":\"",
            "note\":\"",
        ],
    );
}

#[test]
fn generate_debug_diagnostics_single_quiet_records_quiet_stdout_mode() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let out_dir = temp.path().join("images-quiet-diagnostics");
    let diagnostics_path = temp.path().join("quiet-diagnostics.json");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("quiet red circle Bearer sk-quiet b64_json")
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--quiet")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(out_dir.join("manifest.json").is_file());

    let rendered = fs::read_to_string(&diagnostics_path).unwrap();
    let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(diagnostics["metadata"]["result"], "success");
    assert_eq!(diagnostics["metadata"]["stdout_mode"], "quiet");
    assert_eq!(diagnostics["runs"][0]["status"], "succeeded");
    assert_debug_diagnostics_omits(
        &rendered,
        &[
            "quiet red circle",
            "Bearer",
            "sk-quiet",
            "b64_json",
            temp.path().to_string_lossy().as_ref(),
            out_dir.to_string_lossy().as_ref(),
            fake_codex.to_string_lossy().as_ref(),
        ],
    );
}

#[test]
fn generate_debug_diagnostics_single_success_write_failure_maps_to_filesystem_without_stdout() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let out_dir = temp.path().join("images-diagnostics-write-failure");
    let parent_file = temp.path().join("not-a-directory");
    fs::write(&parent_file, "not a directory").unwrap();
    let diagnostics_path = parent_file.join("diagnostics.json");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle secret prompt")
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(5));
    assert!(
        output.stdout.is_empty(),
        "stdout must not be emitted if diagnostics writing fails"
    );
    assert!(
        out_dir.join("manifest.json").is_file(),
        "generation artifacts are written before diagnostics closeout"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(envelope["error"]["code"], "filesystem.output_write_failed");

    let parent_file_path = parent_file.to_string_lossy().to_string();
    assert!(
        !stderr.contains(parent_file_path.as_str()),
        "diagnostics write failure must not leak the requested diagnostics path"
    );
    assert!(!stderr.contains("red circle secret prompt"));
}

#[test]
fn generate_debug_diagnostics_single_failure_diag_write_failure_fails_closed_without_leaks() {
    let temp = TempDir::new().unwrap();
    let failing_codex = write_failing_codex(&temp);
    let out_dir = temp
        .path()
        .join("images-generation-failure-diagnostics-write-failure");
    let parent_file = temp.path().join("not-a-directory-generation-failure");
    fs::write(&parent_file, "not a directory").unwrap();
    let diagnostics_path = parent_file.join("diagnostics.json");
    let prompt = "red circle Bearer sk-single-write-failure b64_json";

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg(prompt)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &failing_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        !out_dir.join("manifest.json").exists(),
        "failed generation must not write a success manifest"
    );
    assert!(
        !diagnostics_path.exists(),
        "failed diagnostics write must not leave a partial diagnostics file"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(envelope["error"]["code"], "filesystem.output_write_failed");

    let parent_file_path = parent_file.to_string_lossy().to_string();
    let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
    let out_path = out_dir.to_string_lossy().to_string();
    let fake_codex_path = failing_codex.to_string_lossy().to_string();
    for forbidden in [
        prompt,
        "Bearer secret should not leak",
        "sk-single-write-failure",
        "b64_json",
        "CODEX_IMAGE_CODEX_BIN",
        parent_file_path.as_str(),
        diagnostics_file_path.as_str(),
        out_path.as_str(),
        fake_codex_path.as_str(),
    ] {
        assert!(
            !stderr.contains(forbidden),
            "stderr leaked forbidden diagnostics-write failure data: {forbidden}"
        );
    }
}

#[test]
fn generate_debug_diagnostics_missing_codex_writes_redacted_sidecar_without_env_key() {
    let temp = TempDir::new().unwrap();
    let out_dir = temp.path().join("images-missing-codex-diagnostics");
    let diagnostics_path = temp.path().join("missing-codex-diagnostics.json");
    let missing_codex = temp.path().join("missing-codex-CODEX_IMAGE_CODEX_BIN");
    let prompt = "red circle Bearer sk-missing-codex b64_json";

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg(prompt)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &missing_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(envelope["error"]["code"], "config.codex_cli_unavailable");

    let rendered = fs::read_to_string(&diagnostics_path).unwrap();
    let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(diagnostics["metadata"]["mode"], "single");
    assert_eq!(diagnostics["metadata"]["result"], "failure");
    assert_eq!(
        diagnostics["failure"]["code"],
        "config.codex_cli_unavailable"
    );
    assert!(diagnostics["runs"].as_array().unwrap().is_empty());

    let rendered_and_stderr = format!("{rendered}\n{stderr}");
    let out_path = out_dir.to_string_lossy().to_string();
    let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
    let missing_codex_path = missing_codex.to_string_lossy().to_string();
    for forbidden in [
        prompt,
        "Bearer",
        "sk-missing-codex",
        "b64_json",
        "CODEX_IMAGE_CODEX_BIN",
        out_path.as_str(),
        diagnostics_file_path.as_str(),
        missing_codex_path.as_str(),
    ] {
        assert!(
            !rendered_and_stderr.contains(forbidden),
            "diagnostics or stderr leaked forbidden missing-Codex data: {forbidden}"
        );
    }
}

#[test]
fn generate_debug_diagnostics_single_failure_modes_record_redacted_run_statuses() {
    let temp = TempDir::new().unwrap();
    let cases = [
        (
            "nonzero",
            write_failing_codex(&temp),
            Some(4),
            "api.codex_image_generation_failed",
            "failed",
            "not_observed",
            "Bearer secret should not leak",
        ),
        (
            "missing-final-message",
            write_missing_final_message_codex(&temp),
            Some(6),
            "response_contract.image_generation",
            "final_message_invalid",
            "missing",
            "sk-missing-final-message",
        ),
        (
            "malformed-final-message",
            write_malformed_final_message_codex(&temp),
            Some(6),
            "response_contract.image_generation",
            "final_message_invalid",
            "invalid_json",
            "sk-malformed-final-message",
        ),
    ];

    for (
        label,
        fake_codex,
        expected_exit,
        expected_error_code,
        expected_run_status,
        expected_final_message_status,
        forbidden_from_fake,
    ) in cases
    {
        let out_dir = temp.path().join(format!("images-{label}"));
        let diagnostics_path = temp.path().join(format!("diagnostics-{label}.json"));
        let prompt = format!("red circle {label} Bearer sk-case-prompt b64_json");

        let output = Command::cargo_bin("codex-image")
            .unwrap()
            .arg("generate")
            .arg(&prompt)
            .arg("--out")
            .arg(&out_dir)
            .arg("--output")
            .arg("json")
            .arg("--debug-diagnostics")
            .arg(&diagnostics_path)
            .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), expected_exit, "case {label}");
        assert!(output.stdout.is_empty(), "failure stdout for {label}");

        let stderr = String::from_utf8(output.stderr).unwrap();
        let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
        assert_eq!(
            envelope["error"]["code"], expected_error_code,
            "case {label}"
        );

        let rendered = fs::read_to_string(&diagnostics_path).unwrap();
        let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(diagnostics["metadata"]["result"], "failure", "case {label}");
        assert_eq!(
            diagnostics["failure"]["code"], expected_error_code,
            "case {label}"
        );
        assert_eq!(
            diagnostics["runs"].as_array().unwrap().len(),
            1,
            "case {label}"
        );

        let run = &diagnostics["runs"][0];
        assert_eq!(run["status"], expected_run_status, "case {label}");
        assert_eq!(
            run["final_message"]["status"], expected_final_message_status,
            "case {label}"
        );
        assert_eq!(run["failure"]["code"], expected_error_code, "case {label}");

        let temp_path = temp.path().to_string_lossy().to_string();
        let out_path = out_dir.to_string_lossy().to_string();
        let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
        let fake_codex_path = fake_codex.to_string_lossy().to_string();
        assert_debug_diagnostics_omits(
            &rendered,
            &[
                prompt.as_str(),
                "Bearer",
                "sk-case-prompt",
                "b64_json",
                forbidden_from_fake,
                "CODEX_IMAGE_CODEX_BIN",
                temp_path.as_str(),
                out_path.as_str(),
                diagnostics_file_path.as_str(),
                fake_codex_path.as_str(),
                ".codex-image-last-message-",
                "image_path\":\"",
                "note\":\"",
            ],
        );
        assert!(
            !stderr.contains(prompt.as_str()),
            "stderr prompt leak for {label}"
        );
        assert!(
            !stderr.contains(forbidden_from_fake),
            "stderr fake leak for {label}"
        );
    }
}

#[test]
fn generate_codex_backend_copies_image_and_writes_manifest_json_output_mode() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let out_dir = temp.path().join("images");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle")
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let trimmed = stdout.trim_end();
    assert_eq!(trimmed.lines().count(), 1, "stdout must be one JSON object");

    let manifest: Value = serde_json::from_str(trimmed).unwrap();
    assert_eq!(manifest["prompt"], "red circle");
    assert_eq!(manifest["model"], "gpt-image-2");

    let image_path = std::path::PathBuf::from(manifest["images"][0]["path"].as_str().unwrap());
    assert_eq!(fs::read(&image_path).unwrap(), b"codex-image-bytes");
    assert_eq!(image_path, out_dir.join("image-0001.png"));

    let manifest_path = out_dir.join("manifest.json");
    assert!(manifest_path.is_file());

    let manifest_text = fs::read_to_string(&manifest_path).unwrap();
    for forbidden in ["Bearer", "access-token", "refresh-token", "b64_json"] {
        assert!(!trimmed.contains(forbidden), "stdout leaked {forbidden}");
        assert!(
            !manifest_text.contains(forbidden),
            "manifest leaked {forbidden}"
        );
    }
}

#[test]
fn generate_rejects_untrusted_preexisting_image_path_without_leaking() {
    let temp = TempDir::new().unwrap();
    let stale_source_image = temp.path().join("stale-source.png");
    fs::write(&stale_source_image, b"stale-image-bytes").unwrap();
    wait_until_file_predates_next_invocation(&stale_source_image);

    let fake_codex = write_untrusted_path_codex(&temp, &stale_source_image);
    let out_dir = temp.path().join("images-untrusted");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle secret prompt")
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty(), "failure stdout must stay empty");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr.trim_end().lines().count(),
        1,
        "failure stderr should be one diagnostics envelope"
    );
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(
        envelope["error"]["code"],
        "response_contract.image_generation"
    );

    assert!(
        !out_dir.join("manifest.json").exists(),
        "untrusted source failure must not produce manifest"
    );
    if out_dir.exists() {
        let copied_images = fs::read_dir(&out_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("image-"))
            .count();
        assert_eq!(copied_images, 0, "failure must not copy image artifacts");
        assert_eq!(
            count_last_message_artifacts(&out_dir),
            0,
            "failure must clean hidden final-message artifacts"
        );
    }

    let stale_path = stale_source_image.to_string_lossy().to_string();
    for forbidden in [
        stale_path.as_str(),
        "red circle secret prompt",
        "Bearer",
        "stale source",
    ] {
        assert!(
            !stderr.contains(forbidden),
            "stderr leaked forbidden source-path failure data: {forbidden}"
        );
    }
}

#[test]
fn generate_defaults_to_human_success_output() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let out_dir = temp.path().join("images");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle")
        .arg("--out")
        .arg(&out_dir)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let trimmed = stdout.trim_end();
    assert!(!trimmed.is_empty(), "human output should not be empty");
    assert!(
        !trimmed.trim_start().starts_with('{'),
        "default generate output should be human-readable"
    );

    assert!(trimmed.contains("codex-image generated 1 image"));
    assert!(trimmed.contains("model: gpt-image-2"));
    assert!(trimmed.contains(&format!("out: {}", out_dir.display())));
    assert!(trimmed.contains("manifest:"));
    assert!(trimmed.contains("image[1]:"));
}

#[test]
fn generate_quiet_success_suppresses_stdout_but_still_writes_artifacts() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let out_dir = temp.path().join("images");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle")
        .arg("--out")
        .arg(&out_dir)
        .arg("--quiet")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let manifest_path = out_dir.join("manifest.json");
    assert!(manifest_path.is_file());
    let manifest_text = fs::read_to_string(&manifest_path).unwrap();
    let manifest: Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest["prompt"], "red circle");
}

#[test]
fn generate_output_json_with_quiet_suppresses_stdout_and_still_writes_manifest() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let out_dir = temp.path().join("images");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle")
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--quiet")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let manifest_path = out_dir.join("manifest.json");
    assert!(manifest_path.is_file());
    let manifest_text = fs::read_to_string(&manifest_path).unwrap();
    let manifest: Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest["prompt"], "red circle");
}

#[test]
fn generate_codex_failure_with_output_flags_maps_to_redacted_json_envelope() {
    let temp = TempDir::new().unwrap();
    let failing_codex = write_failing_codex(&temp);

    for (label, args) in [
        ("quiet", vec!["--quiet"]),
        ("output-json", vec!["--output", "json"]),
        ("output-json-quiet", vec!["--output", "json", "--quiet"]),
    ] {
        let out_dir = temp.path().join(format!("images-{label}"));
        let mut cmd = Command::cargo_bin("codex-image").unwrap();
        cmd.arg("generate")
            .arg("red circle")
            .arg("--out")
            .arg(&out_dir)
            .env("CODEX_IMAGE_CODEX_BIN", &failing_codex);
        for arg in args {
            cmd.arg(arg);
        }

        let output = cmd.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(4),
            "expected api failure exit for {label}"
        );
        assert!(
            output.stdout.is_empty(),
            "stdout should stay empty on failure for {label}"
        );

        let stderr = String::from_utf8(output.stderr).unwrap();
        let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
        assert_eq!(
            envelope["error"]["code"],
            "api.codex_image_generation_failed"
        );
        assert!(!stderr.contains("Bearer"));
        assert!(!stderr.contains("secret"));
    }
}

#[test]
fn generate_timeout_maps_to_redacted_failure_and_cleans_hidden_artifacts() {
    let temp = TempDir::new().unwrap();
    let sleep_secs = 6_u64;
    let token_fixture = "sk-timeout-fixture-1234567890";
    let sleeping_codex = write_sleeping_codex(&temp, sleep_secs, token_fixture);
    let out_dir = temp.path().join("images-timeout");

    let started_at = Instant::now();
    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle")
        .arg("--out")
        .arg(&out_dir)
        .arg("--timeout")
        .arg("1")
        .arg("--output")
        .arg("json")
        .env("CODEX_IMAGE_CODEX_BIN", &sleeping_codex)
        .output()
        .unwrap();
    let elapsed = started_at.elapsed();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr.trim_end().lines().count(),
        1,
        "failure stderr should be a single diagnostics envelope"
    );

    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(
        envelope["error"]["code"],
        "api.codex_image_generation_failed"
    );

    for forbidden in ["red circle", "Bearer", "secret", token_fixture] {
        assert!(
            !stderr.contains(forbidden),
            "stderr leaked forbidden timeout data: {forbidden}"
        );
    }

    assert!(
        elapsed < Duration::from_secs(sleep_secs - 1),
        "timeout should end quickly (elapsed: {:?}, sleep: {sleep_secs}s)",
        elapsed
    );

    assert!(
        !out_dir.join("manifest.json").exists(),
        "timeout failure must not produce manifest"
    );

    if out_dir.exists() {
        let copied_images = fs::read_dir(&out_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("image-"))
            .count();
        assert_eq!(
            copied_images, 0,
            "timeout failure must not copy image artifacts"
        );
        assert_eq!(
            count_last_message_artifacts(&out_dir),
            0,
            "timeout failure must clean hidden final-message artifacts"
        );
    }
}

#[test]
fn generate_debug_diagnostics_timeout_failure_writes_redacted_sidecar() {
    let temp = TempDir::new().unwrap();
    let sleep_secs = 6_u64;
    let token_fixture = "sk-timeout-diagnostics-fixture-1234567890";
    let sleeping_codex = write_sleeping_codex(&temp, sleep_secs, token_fixture);
    let out_dir = temp.path().join("images-timeout-diagnostics");
    let diagnostics_path = temp.path().join("timeout-diagnostics.json");
    let prompt = "red circle Bearer timeout prompt";

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg(prompt)
        .arg("--out")
        .arg(&out_dir)
        .arg("--timeout")
        .arg("1")
        .arg("--output")
        .arg("json")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &sleeping_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(
        envelope["error"]["code"],
        "api.codex_image_generation_failed"
    );

    let rendered = fs::read_to_string(&diagnostics_path).unwrap();
    let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(diagnostics["metadata"]["mode"], "single");
    assert_eq!(diagnostics["metadata"]["result"], "failure");
    assert_eq!(diagnostics["metadata"]["stdout_mode"], "json");
    assert_eq!(
        diagnostics["failure"]["code"],
        "api.codex_image_generation_failed"
    );

    let run = &diagnostics["runs"][0];
    assert_eq!(run["item_index"], Value::Null);
    assert_eq!(run["phase"], "generate_image");
    assert_eq!(run["outcome"], "timed_out");
    assert_eq!(run["status"], "timed_out");
    assert_eq!(run["timeout_seconds"], 1);
    assert_eq!(run["timed_out"], true);
    assert!(run.get("exit_code").is_none());
    assert_eq!(run["final_message"]["status"], "not_observed");
    assert_eq!(run["final_message"]["presence"], "not_observed");
    assert_eq!(run["final_message"]["parse"], "not_attempted");
    assert_eq!(run["failure"]["code"], "api.codex_image_generation_failed");

    let temp_path = temp.path().to_string_lossy().to_string();
    let out_path = out_dir.to_string_lossy().to_string();
    let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
    let sleeping_codex_path = sleeping_codex.to_string_lossy().to_string();
    assert_debug_diagnostics_omits(
        &rendered,
        &[
            prompt,
            "Bearer",
            "secret",
            token_fixture,
            "b64_json",
            "CODEX_IMAGE_CODEX_BIN",
            temp_path.as_str(),
            out_path.as_str(),
            diagnostics_file_path.as_str(),
            sleeping_codex_path.as_str(),
            ".codex-image-last-message-",
            "image_path\":\"",
            "note\":\"",
        ],
    );
}

#[test]
fn generate_debug_diagnostics_batch_success_writes_progress_runs_and_sanitizes_sidecar() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    let second_source_image = temp.path().join("codex-source-two.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();
    fs::write(&second_source_image, b"second-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-diagnostics-success-argv.log");
    let (fake_codex, _counter_file) = write_batch_arg_recording_codex(
        &temp,
        &first_source_image,
        &second_source_image,
        &argv_log,
    );

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(
        &prompt_file,
        "first prompt Bearer sk-batch-success b64_json\nsecond prompt secret\n",
    )
    .unwrap();

    let out_dir = temp.path().join("out");
    let diagnostics_path = temp.path().join("nested").join("batch-diagnostics.json");
    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--timeout")
        .arg("7")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stdout_manifest: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(stdout_manifest["mode"], "batch");
    assert_eq!(stdout_manifest["item_count"], 2);
    assert!(out_dir.join("manifest.json").is_file());

    let rendered = fs::read_to_string(&diagnostics_path).unwrap();
    let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(diagnostics["schema"], "codex-image.generation-diagnostics");
    assert_eq!(diagnostics["schema_version"], 1);
    assert_eq!(diagnostics["metadata"]["mode"], "batch");
    assert_eq!(diagnostics["metadata"]["result"], "success");
    assert_eq!(diagnostics["metadata"]["prompt_source"], "prompt_file");
    assert_eq!(diagnostics["metadata"]["stdout_mode"], "json");
    assert_eq!(diagnostics["metadata"]["timeout_seconds"], 7);
    assert!(diagnostics.get("failure").is_none());
    assert_eq!(diagnostics["batch"]["planned_items"], 2);
    assert_eq!(diagnostics["batch"]["completed_items"], 2);
    assert_eq!(diagnostics["batch"]["failed_item_index"], Value::Null);

    let runs = diagnostics["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    for (zero_based, run) in runs.iter().enumerate() {
        let expected_index = zero_based + 1;
        assert_eq!(run["index"], expected_index);
        assert_eq!(run["item_index"], expected_index);
        assert_eq!(run["phase"], "generate_image");
        assert_eq!(run["outcome"], "succeeded");
        assert_eq!(run["status"], "succeeded");
        assert_eq!(run["timeout_seconds"], 7);
        assert_eq!(run["timed_out"], false);
        assert_eq!(run["exit_code"], 0);
        assert_eq!(run["final_message"]["status"], "parsed");
        assert!(run.get("failure").is_none());
    }

    let temp_path = temp.path().to_string_lossy().to_string();
    let prompt_file_path = prompt_file.to_string_lossy().to_string();
    let out_path = out_dir.to_string_lossy().to_string();
    let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
    let fake_codex_path = fake_codex.to_string_lossy().to_string();
    let first_source_path = first_source_image.to_string_lossy().to_string();
    let second_source_path = second_source_image.to_string_lossy().to_string();
    assert_debug_diagnostics_omits(
        &rendered,
        &[
            "first prompt",
            "second prompt",
            "Bearer",
            "sk-batch-success",
            "b64_json",
            "CODEX_IMAGE_CODEX_BIN",
            temp_path.as_str(),
            prompt_file_path.as_str(),
            out_path.as_str(),
            diagnostics_file_path.as_str(),
            fake_codex_path.as_str(),
            first_source_path.as_str(),
            second_source_path.as_str(),
            ".codex-image-last-message-",
            "image_path\":\"",
            "note\":\"",
        ],
    );
}

#[test]
fn generate_debug_diagnostics_batch_failure_reports_partial_progress_and_redacts_sidecar() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-diagnostics-failure-argv.log");
    let token_sentinel = "sk-batch-diagnostics-fail-fixture-12345";
    let (fake_codex, counter_file) =
        write_batch_second_fail_codex(&temp, &first_source_image, &argv_log, token_sentinel);

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(
        &prompt_file,
        "first prompt\nsecond prompt secret\nthird prompt\n",
    )
    .unwrap();

    let out_dir = temp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("manifest.json"), "{\"stale\":true}").unwrap();
    let diagnostics_path = temp.path().join("batch-failure-diagnostics.json");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty(), "failure stdout must stay empty");

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(
        envelope["error"]["code"],
        "api.codex_image_generation_failed"
    );
    assert!(
        !out_dir.join("manifest.json").exists(),
        "stale root manifest must be removed and not rewritten on partial failure"
    );
    assert!(out_dir.join("item-0001").join("manifest.json").is_file());
    assert!(
        !out_dir.join("item-0003").exists(),
        "batch must stop before the third item"
    );

    let call_count: usize = fs::read_to_string(&counter_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(call_count, 2, "Codex should run exactly twice");

    let rendered = fs::read_to_string(&diagnostics_path).unwrap();
    let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(diagnostics["metadata"]["mode"], "batch");
    assert_eq!(diagnostics["metadata"]["result"], "failure");
    assert_eq!(diagnostics["metadata"]["prompt_source"], "prompt_file");
    assert_eq!(diagnostics["metadata"]["stdout_mode"], "json");
    assert_eq!(
        diagnostics["failure"]["code"],
        "api.codex_image_generation_failed"
    );
    assert_eq!(diagnostics["batch"]["planned_items"], 3);
    assert_eq!(diagnostics["batch"]["completed_items"], 1);
    assert_eq!(diagnostics["batch"]["failed_item_index"], 2);

    let runs = diagnostics["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2, "third item must not produce a run entry");
    assert_eq!(runs[0]["index"], 1);
    assert_eq!(runs[0]["item_index"], 1);
    assert_eq!(runs[0]["status"], "succeeded");
    assert_eq!(runs[1]["index"], 2);
    assert_eq!(runs[1]["item_index"], 2);
    assert_eq!(runs[1]["outcome"], "failed");
    assert_eq!(runs[1]["status"], "failed");
    assert_eq!(runs[1]["final_message"]["status"], "not_observed");
    assert_eq!(
        runs[1]["failure"]["code"],
        "api.codex_image_generation_failed"
    );

    let prompt_file_path = prompt_file.to_string_lossy().to_string();
    let out_dir_path = out_dir.to_string_lossy().to_string();
    let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
    let fake_codex_path = fake_codex.to_string_lossy().to_string();
    let first_source_path = first_source_image.to_string_lossy().to_string();
    assert_debug_diagnostics_omits(
        &rendered,
        &[
            "first prompt",
            "second prompt secret",
            "third prompt",
            token_sentinel,
            "/tmp/sensitive/path.txt",
            "Bearer",
            "CODEX_IMAGE_CODEX_BIN",
            prompt_file_path.as_str(),
            out_dir_path.as_str(),
            diagnostics_file_path.as_str(),
            fake_codex_path.as_str(),
            first_source_path.as_str(),
            ".codex-image-last-message-",
            "image_path\":\"",
            "note\":\"",
        ],
    );
    for forbidden in [
        "second prompt secret",
        token_sentinel,
        "/tmp/sensitive/path.txt",
        "Bearer",
        prompt_file_path.as_str(),
        out_dir_path.as_str(),
    ] {
        assert!(
            !stderr.contains(forbidden),
            "stderr leaked forbidden batch failure data: {forbidden}"
        );
    }
}

#[test]
fn generate_debug_diagnostics_batch_failure_diag_write_failure_fails_closed_without_leaks() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-diagnostics-write-failure-argv.log");
    let token_sentinel = "sk-batch-diagnostics-write-failure-fixture-12345";
    let (fake_codex, counter_file) =
        write_batch_second_fail_codex(&temp, &first_source_image, &argv_log, token_sentinel);

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(
        &prompt_file,
        "first prompt\nsecond prompt Bearer secret\nthird prompt\n",
    )
    .unwrap();

    let out_dir = temp.path().join("out-batch-diagnostics-write-failure");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("manifest.json"), "{\"stale\":true}").unwrap();
    let parent_file = temp.path().join("not-a-directory-batch-diagnostics");
    fs::write(&parent_file, "not a directory").unwrap();
    let diagnostics_path = parent_file.join("diagnostics.json");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty(), "failure stdout must stay empty");
    assert!(
        !out_dir.join("manifest.json").exists(),
        "stale root manifest must be removed and not rewritten on partial failure"
    );
    assert!(out_dir.join("item-0001").join("manifest.json").is_file());
    assert!(
        !out_dir.join("item-0003").exists(),
        "batch must stop before the third item"
    );
    assert!(
        !diagnostics_path.exists(),
        "failed diagnostics write must not leave a partial diagnostics file"
    );

    let call_count: usize = fs::read_to_string(&counter_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(call_count, 2, "Codex should run exactly twice");

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(envelope["error"]["code"], "filesystem.output_write_failed");

    let prompt_file_path = prompt_file.to_string_lossy().to_string();
    let out_dir_path = out_dir.to_string_lossy().to_string();
    let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
    let parent_file_path = parent_file.to_string_lossy().to_string();
    let fake_codex_path = fake_codex.to_string_lossy().to_string();
    let first_source_path = first_source_image.to_string_lossy().to_string();
    for forbidden in [
        "first prompt",
        "second prompt Bearer secret",
        "third prompt",
        token_sentinel,
        "/tmp/sensitive/path.txt",
        "Bearer",
        "CODEX_IMAGE_CODEX_BIN",
        prompt_file_path.as_str(),
        out_dir_path.as_str(),
        diagnostics_file_path.as_str(),
        parent_file_path.as_str(),
        fake_codex_path.as_str(),
        first_source_path.as_str(),
    ] {
        assert!(
            !stderr.contains(forbidden),
            "stderr leaked forbidden batch diagnostics-write failure data: {forbidden}"
        );
    }
}

#[test]
fn generate_debug_diagnostics_batch_prompt_file_failures_write_zero_run_sidecars() {
    let temp = TempDir::new().unwrap();
    let missing_prompt_file = temp.path().join("missing-prompts.txt");
    let empty_prompt_file = temp.path().join("empty-prompts.txt");
    fs::write(&empty_prompt_file, "\n# comment only\n   \n").unwrap();

    let cases = [
        (
            "missing",
            missing_prompt_file,
            5,
            "filesystem.prompt_file_read_failed",
        ),
        ("empty", empty_prompt_file, 2, "usage.prompt_file_empty"),
    ];

    for (label, prompt_file, expected_exit, expected_error_code) in cases {
        let out_dir = temp.path().join(format!("out-{label}"));
        let diagnostics_path = temp.path().join(format!("diagnostics-{label}.json"));

        let output = Command::cargo_bin("codex-image")
            .unwrap()
            .arg("generate")
            .arg("--prompt-file")
            .arg(&prompt_file)
            .arg("--out")
            .arg(&out_dir)
            .arg("--output")
            .arg("json")
            .arg("--debug-diagnostics")
            .arg(&diagnostics_path)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(expected_exit), "case {label}");
        assert!(output.stdout.is_empty(), "case {label}");

        let stderr = String::from_utf8(output.stderr).unwrap();
        let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
        assert_eq!(
            envelope["error"]["code"], expected_error_code,
            "case {label}"
        );

        let rendered = fs::read_to_string(&diagnostics_path).unwrap();
        let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(diagnostics["metadata"]["mode"], "batch", "case {label}");
        assert_eq!(diagnostics["metadata"]["result"], "failure", "case {label}");
        assert_eq!(
            diagnostics["metadata"]["prompt_source"], "prompt_file",
            "case {label}"
        );
        assert_eq!(
            diagnostics["failure"]["code"], expected_error_code,
            "case {label}"
        );
        assert_eq!(diagnostics["batch"]["planned_items"], 0, "case {label}");
        assert_eq!(diagnostics["batch"]["completed_items"], 0, "case {label}");
        assert_eq!(
            diagnostics["batch"]["failed_item_index"],
            Value::Null,
            "case {label}"
        );
        assert!(
            diagnostics["runs"].as_array().unwrap().is_empty(),
            "case {label}"
        );

        let prompt_file_path = prompt_file.to_string_lossy().to_string();
        let out_path = out_dir.to_string_lossy().to_string();
        let diagnostics_file_path = diagnostics_path.to_string_lossy().to_string();
        assert_debug_diagnostics_omits(
            &rendered,
            &[
                "comment only",
                prompt_file_path.as_str(),
                out_path.as_str(),
                diagnostics_file_path.as_str(),
                temp.path().to_string_lossy().as_ref(),
            ],
        );
        for forbidden in ["comment only", prompt_file_path.as_str(), out_path.as_str()] {
            assert!(
                !stderr.contains(forbidden),
                "stderr leaked forbidden prompt-file failure data: {forbidden}"
            );
        }
    }
}

#[test]
fn generate_debug_diagnostics_batch_quiet_success_records_quiet_without_stdout() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    let second_source_image = temp.path().join("codex-source-two.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();
    fs::write(&second_source_image, b"second-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-diagnostics-quiet-argv.log");
    let (fake_codex, _counter_file) = write_batch_arg_recording_codex(
        &temp,
        &first_source_image,
        &second_source_image,
        &argv_log,
    );

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first prompt\nsecond prompt\n").unwrap();
    let out_dir = temp.path().join("out");
    let diagnostics_path = temp.path().join("quiet-batch-diagnostics.json");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--quiet")
        .arg("--debug-diagnostics")
        .arg(&diagnostics_path)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "quiet success must suppress stdout"
    );
    assert!(output.stderr.is_empty());
    assert!(out_dir.join("manifest.json").is_file());

    let rendered = fs::read_to_string(&diagnostics_path).unwrap();
    let diagnostics: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(diagnostics["metadata"]["mode"], "batch");
    assert_eq!(diagnostics["metadata"]["result"], "success");
    assert_eq!(diagnostics["metadata"]["stdout_mode"], "quiet");
    assert_eq!(diagnostics["batch"]["planned_items"], 2);
    assert_eq!(diagnostics["batch"]["completed_items"], 2);
    assert_eq!(diagnostics["runs"].as_array().unwrap().len(), 2);
}

#[test]
fn generate_timeout_is_local_only_and_not_forwarded_to_codex_subprocess() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let argv_log = temp.path().join("codex-argv.log");
    let fake_codex = write_arg_recording_codex(&temp, &source_image, &argv_log);
    let out_dir = temp.path().join("images-timeout-local");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("timeout forwarding probe")
        .arg("--out")
        .arg(&out_dir)
        .arg("--timeout")
        .arg("7")
        .arg("--output")
        .arg("json")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let manifest: Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim_end()).unwrap();
    assert_eq!(manifest["prompt"], "timeout forwarding probe");

    let argv = fs::read_to_string(&argv_log).unwrap();
    let argv_tokens: Vec<&str> = argv.lines().collect();
    assert!(
        !argv_tokens.contains(&"--timeout"),
        "timeout flag must not be forwarded to Codex subprocess"
    );
    assert!(
        !argv_tokens.contains(&"7"),
        "timeout value must not be forwarded to Codex subprocess"
    );
}

#[test]
fn generate_batch_success_from_prompt_file_writes_item_outputs_and_root_json_contract() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    let second_source_image = temp.path().join("codex-source-two.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();
    fs::write(&second_source_image, b"second-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-codex-argv.log");
    let (fake_codex, _counter_file) = write_batch_arg_recording_codex(
        &temp,
        &first_source_image,
        &second_source_image,
        &argv_log,
    );

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(
        &prompt_file,
        "\n# top comment\r\n   # indented comment\r\n first prompt  \r\n\r\nsecond prompt\t\n",
    )
    .unwrap();

    let out_dir = temp.path().join("out");
    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--timeout")
        .arg("7")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let trimmed = stdout.trim_end();
    assert_eq!(trimmed.lines().count(), 1, "stdout must be one JSON object");

    let stdout_manifest: Value = serde_json::from_str(trimmed).unwrap();
    assert_eq!(stdout_manifest["mode"], "batch");
    assert_eq!(stdout_manifest["item_count"], 2);
    assert_eq!(stdout_manifest["items"][0]["prompt"], "first prompt");
    assert_eq!(stdout_manifest["items"][1]["prompt"], "second prompt");

    let root_manifest_path = out_dir.join("manifest.json");
    assert!(root_manifest_path.is_file(), "root manifest should exist");
    let root_manifest: Value =
        serde_json::from_str(&fs::read_to_string(&root_manifest_path).unwrap()).unwrap();
    assert_eq!(root_manifest, stdout_manifest);

    assert_eq!(
        root_manifest["prompt_file"],
        prompt_file.to_string_lossy().to_string()
    );

    let item_one_dir = out_dir.join("item-0001");
    let item_two_dir = out_dir.join("item-0002");
    let item_three_dir = out_dir.join("item-0003");

    assert!(item_one_dir.is_dir());
    assert!(item_two_dir.is_dir());
    assert!(!item_three_dir.exists());

    let item_one_image = item_one_dir.join("image-0001.png");
    let item_two_image = item_two_dir.join("image-0001.png");
    assert!(item_one_image.is_file());
    assert!(item_two_image.is_file());
    assert!(item_one_dir.join("manifest.json").is_file());
    assert!(item_two_dir.join("manifest.json").is_file());
    assert_eq!(fs::read(item_one_image).unwrap(), b"first-image-bytes");
    assert_eq!(fs::read(item_two_image).unwrap(), b"second-image-bytes");

    let argv = fs::read_to_string(&argv_log).unwrap();
    assert_eq!(argv.matches("CALL ").count(), 2, "Codex should run twice");
    let argv_tokens: Vec<&str> = argv.lines().collect();
    assert!(
        !argv_tokens.contains(&"--timeout"),
        "timeout flag must not be forwarded to Codex subprocess"
    );
    assert!(
        !argv_tokens.contains(&"7"),
        "timeout value must not be forwarded to Codex subprocess"
    );
}

#[test]
fn generate_batch_rejects_untrusted_preexisting_image_path_without_root_manifest() {
    let temp = TempDir::new().unwrap();
    let stale_source_image = temp.path().join("stale-batch-source.png");
    fs::write(&stale_source_image, b"stale-batch-image-bytes").unwrap();
    wait_until_file_predates_next_invocation(&stale_source_image);

    let fake_codex = write_untrusted_path_codex(&temp, &stale_source_image);
    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first prompt secret\nsecond prompt secret\n").unwrap();

    let out_dir = temp.path().join("out-untrusted");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("manifest.json"), "{\"stale\":true}").unwrap();

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty(), "failure stdout must stay empty");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr.trim_end().lines().count(),
        1,
        "failure stderr should be one diagnostics envelope"
    );
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(
        envelope["error"]["code"],
        "response_contract.image_generation"
    );

    assert!(
        !out_dir.join("manifest.json").exists(),
        "stale root manifest must be removed and not rewritten"
    );
    assert!(
        !out_dir.join("item-0001").join("manifest.json").exists(),
        "failed first batch item must not write an item manifest"
    );
    assert!(
        !out_dir.join("item-0001").join("image-0001.png").exists(),
        "failed first batch item must not copy an image"
    );
    assert!(
        !out_dir.join("item-0002").exists(),
        "batch must stop after untrusted first item"
    );

    let stale_path = stale_source_image.to_string_lossy().to_string();
    let prompt_file_path = prompt_file.to_string_lossy().to_string();
    for forbidden in [
        stale_path.as_str(),
        prompt_file_path.as_str(),
        "first prompt secret",
        "second prompt secret",
        "Bearer",
        "stale source",
    ] {
        assert!(
            !stderr.contains(forbidden),
            "stderr leaked forbidden batch source-path failure data: {forbidden}"
        );
    }
}

#[test]
fn generate_batch_failure_stops_on_second_item_and_removes_stale_root_manifest() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-second-fail-argv.log");
    let token_sentinel = "sk-batch-fail-fixture-12345";
    let (fake_codex, counter_file) =
        write_batch_second_fail_codex(&temp, &first_source_image, &argv_log, token_sentinel);

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first prompt\nsecond prompt\nthird prompt\n").unwrap();

    let out_dir = temp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("manifest.json"), "{\"stale\":true}").unwrap();

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(4),
        "second-item Codex failure should map to API exit code"
    );
    assert!(output.stdout.is_empty(), "failure stdout must stay empty");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr.trim_end().lines().count(),
        1,
        "failure stderr should be one diagnostics envelope"
    );
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(
        envelope["error"]["code"],
        "api.codex_image_generation_failed"
    );

    assert!(
        !out_dir.join("manifest.json").exists(),
        "stale root manifest must be removed and not rewritten on partial failure"
    );
    assert!(
        out_dir.join("item-0001").join("manifest.json").is_file(),
        "first item artifacts should remain"
    );
    assert!(
        !out_dir.join("item-0003").exists(),
        "batch must stop after second-item failure"
    );

    let call_count: usize = fs::read_to_string(&counter_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(call_count, 2, "Codex should run exactly twice");

    let argv = fs::read_to_string(&argv_log).unwrap();
    assert_eq!(
        argv.matches("CALL ").count(),
        2,
        "argv log should show two runs"
    );

    let prompt_file_path = prompt_file.to_string_lossy().to_string();
    let out_dir_path = out_dir.to_string_lossy().to_string();
    for forbidden in [
        "second prompt",
        token_sentinel,
        "/tmp/sensitive/path.txt",
        "Bearer",
        prompt_file_path.as_str(),
        out_dir_path.as_str(),
    ] {
        assert!(
            !stderr.contains(forbidden),
            "stderr leaked forbidden batch failure data: {forbidden}"
        );
    }
}

#[test]
fn generate_batch_defaults_to_human_success_output() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    let second_source_image = temp.path().join("codex-source-two.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();
    fs::write(&second_source_image, b"second-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-human-argv.log");
    let (fake_codex, _counter_file) = write_batch_arg_recording_codex(
        &temp,
        &first_source_image,
        &second_source_image,
        &argv_log,
    );

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first prompt\nsecond prompt\n").unwrap();

    let out_dir = temp.path().join("out");
    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let trimmed = stdout.trim_end();
    assert!(!trimmed.is_empty(), "human output should not be empty");
    assert!(
        !trimmed.trim_start().starts_with('{'),
        "batch default output should be human-readable"
    );

    let root_manifest = out_dir.join("manifest.json");
    let item_one_manifest = out_dir.join("item-0001").join("manifest.json");
    let item_two_manifest = out_dir.join("item-0002").join("manifest.json");
    let item_one_manifest_path = item_one_manifest.to_string_lossy().to_string();
    let item_two_manifest_path = item_two_manifest.to_string_lossy().to_string();

    assert!(trimmed.contains("codex-image generated 2 batch items"));
    assert!(trimmed.contains(&format!("manifest: {}", root_manifest.display())));
    assert!(trimmed.contains(&item_one_manifest_path));
    assert!(trimmed.contains(&item_two_manifest_path));

    assert!(root_manifest.is_file());
    assert!(item_one_manifest.is_file());
    assert!(item_two_manifest.is_file());
}

#[test]
fn generate_batch_quiet_success_suppresses_stdout_but_writes_manifests() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    let second_source_image = temp.path().join("codex-source-two.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();
    fs::write(&second_source_image, b"second-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-quiet-argv.log");
    let (fake_codex, _counter_file) = write_batch_arg_recording_codex(
        &temp,
        &first_source_image,
        &second_source_image,
        &argv_log,
    );

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first prompt\nsecond prompt\n").unwrap();

    let out_dir = temp.path().join("out");
    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--quiet")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "quiet success must suppress stdout"
    );
    assert!(output.stderr.is_empty());

    assert!(out_dir.join("manifest.json").is_file());
    assert!(out_dir.join("item-0001").join("manifest.json").is_file());
    assert!(out_dir.join("item-0002").join("manifest.json").is_file());
}

#[test]
fn generate_batch_output_json_with_quiet_suppresses_stdout_but_writes_manifests() {
    let temp = TempDir::new().unwrap();
    let first_source_image = temp.path().join("codex-source-one.png");
    let second_source_image = temp.path().join("codex-source-two.png");
    fs::write(&first_source_image, b"first-image-bytes").unwrap();
    fs::write(&second_source_image, b"second-image-bytes").unwrap();

    let argv_log = temp.path().join("batch-json-quiet-argv.log");
    let (fake_codex, _counter_file) = write_batch_arg_recording_codex(
        &temp,
        &first_source_image,
        &second_source_image,
        &argv_log,
    );

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first prompt\nsecond prompt\n").unwrap();

    let out_dir = temp.path().join("out");
    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--out")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .arg("--quiet")
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "quiet success must suppress stdout"
    );
    assert!(output.stderr.is_empty());

    assert!(out_dir.join("manifest.json").is_file());
    assert!(out_dir.join("item-0001").join("manifest.json").is_file());
    assert!(out_dir.join("item-0002").join("manifest.json").is_file());
}

#[test]
fn generate_missing_codex_maps_to_config_error() {
    let temp = TempDir::new().unwrap();
    let out_dir = temp.path().join("images");

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("red circle")
        .arg("--out")
        .arg(&out_dir)
        .env("CODEX_IMAGE_CODEX_BIN", temp.path().join("missing-codex"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(envelope["error"]["code"], "config.codex_cli_unavailable");
}

#[test]
fn generate_filesystem_failure_maps_to_exit_5_when_out_is_existing_file() {
    let temp = TempDir::new().unwrap();
    let source_image = temp.path().join("codex-source.png");
    fs::write(&source_image, b"codex-image-bytes").unwrap();
    let fake_codex = write_fake_codex(&temp, &source_image);
    let existing_file = NamedTempFile::new().unwrap();

    let output = Command::cargo_bin("codex-image")
        .unwrap()
        .arg("generate")
        .arg("filesystem fail")
        .arg("--out")
        .arg(existing_file.path())
        .env("CODEX_IMAGE_CODEX_BIN", &fake_codex)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8(output.stderr).unwrap();
    let envelope: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(envelope["error"]["code"], "filesystem.output_write_failed");
}

#[test]
fn generate_clap_usage_errors_emit_no_json_envelope() {
    let mut missing_prompt_source = Command::cargo_bin("codex-image").unwrap();
    missing_prompt_source
        .arg("generate")
        .arg("--out")
        .arg("./images");

    missing_prompt_source
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("--prompt-file")
                .or(predicate::str::contains("<PROMPT>"))
                .or(predicate::str::contains("<prompt>")),
        )
        .stderr(predicate::str::contains("\"error\":").not());

    let mut missing_out = Command::cargo_bin("codex-image").unwrap();
    missing_out.arg("generate").arg("prompt only");

    missing_out
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--out"))
        .stderr(predicate::str::contains("\"error\":").not());
}
