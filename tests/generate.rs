use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

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
printf '{{"image_path":"{}","note":"fake codex generated image"}}' > "$last_message"
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
        !argv_tokens.iter().any(|arg| *arg == "--timeout"),
        "timeout flag must not be forwarded to Codex subprocess"
    );
    assert!(
        !argv_tokens.iter().any(|arg| *arg == "7"),
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
        !argv_tokens.iter().any(|arg| *arg == "--timeout"),
        "timeout flag must not be forwarded to Codex subprocess"
    );
    assert!(
        !argv_tokens.iter().any(|arg| *arg == "7"),
        "timeout value must not be forwarded to Codex subprocess"
    );
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
    assert_eq!(argv.matches("CALL ").count(), 2, "argv log should show two runs");

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
    assert!(output.stdout.is_empty(), "quiet success must suppress stdout");
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
    assert!(output.stdout.is_empty(), "quiet success must suppress stdout");
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
