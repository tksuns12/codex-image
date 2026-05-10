use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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
    let mut missing_prompt = Command::cargo_bin("codex-image").unwrap();
    missing_prompt.arg("generate").arg("--out").arg("./images");

    missing_prompt
        .assert()
        .code(2)
        .stderr(predicate::str::contains("<PROMPT>").or(predicate::str::contains("<prompt>")))
        .stderr(predicate::str::contains("\"error\":").not());

    let mut missing_out = Command::cargo_bin("codex-image").unwrap();
    missing_out.arg("generate").arg("prompt only");

    missing_out
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--out"))
        .stderr(predicate::str::contains("\"error\":").not());
}
