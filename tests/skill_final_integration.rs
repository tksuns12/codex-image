use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use codex_image::skill_installer::render_managed_skill_content;
use codex_image::skills::{resolve_skill_path, SkillScope, SupportedTool};
use serde_json::Value;
use tempfile::tempdir;

#[derive(Debug)]
struct ExpectedTarget {
    tool: SupportedTool,
    scope: SkillScope,
    path: PathBuf,
}

fn parse_json_object(bytes: &[u8]) -> Value {
    let text = String::from_utf8(bytes.to_vec()).expect("stdout should be utf-8");
    let trimmed = text.trim_end();
    assert!(!trimmed.is_empty(), "stdout must not be empty");
    assert_eq!(trimmed.lines().count(), 1, "stdout must be one JSON object");
    serde_json::from_str::<Value>(trimmed).expect("stdout should be valid JSON")
}

fn parse_single_json_line(bytes: &[u8]) -> Value {
    parse_json_object(bytes)
}

fn results(value: &Value) -> &[Value] {
    value["results"]
        .as_array()
        .expect("payload should contain results array")
}

fn single_result(value: &Value) -> &Value {
    let results = results(value);
    assert_eq!(results.len(), 1, "payload should contain one result");
    &results[0]
}

fn expected_targets(home_dir: &Path, project_root: &Path) -> Vec<ExpectedTarget> {
    let mut targets = Vec::new();
    for tool in SupportedTool::all() {
        for scope in SkillScope::all() {
            targets.push(ExpectedTarget {
                tool,
                scope,
                path: resolve_skill_path(tool, scope, home_dir, project_root),
            });
        }
    }

    targets
}

fn expected_install_statuses(expected: &[ExpectedTarget]) -> Vec<String> {
    let mut seen_paths = HashSet::<PathBuf>::new();
    expected
        .iter()
        .map(|target| {
            if seen_paths.insert(target.path.clone()) {
                "created".to_string()
            } else {
                "unchanged".to_string()
            }
        })
        .collect()
}

fn expected_unchanged_statuses(expected: &[ExpectedTarget]) -> Vec<String> {
    vec!["unchanged".to_string(); expected.len()]
}

fn run_skill_command(
    project_root: &Path,
    home_dir: &Path,
    operation: &str,
) -> std::process::Output {
    let mut cmd = Command::cargo_bin("codex-image").expect("binary exists");
    cmd.current_dir(project_root)
        .arg("skill")
        .arg(operation)
        .arg("--tool")
        .arg("claude")
        .arg("--tool")
        .arg("claude-code")
        .arg("--tool")
        .arg("codex")
        .arg("--tool")
        .arg("pi")
        .arg("--tool")
        .arg("opencode")
        .arg("--scope")
        .arg("global")
        .arg("--scope")
        .arg("project")
        .arg("--yes")
        .arg("--output")
        .arg("json")
        .env("HOME", home_dir);

    cmd.output().expect("command should run")
}

fn run_single_target_skill_command(
    project_root: &Path,
    home_dir: &Path,
    operation: &str,
    tool: &str,
    scope: &str,
    force: bool,
) -> std::process::Output {
    let mut cmd = Command::cargo_bin("codex-image").expect("binary exists");
    cmd.current_dir(project_root)
        .arg("skill")
        .arg(operation)
        .arg("--tool")
        .arg(tool)
        .arg("--scope")
        .arg(scope)
        .arg("--yes")
        .arg("--output")
        .arg("json")
        .env("HOME", home_dir);

    if force {
        cmd.arg("--force");
    }

    cmd.output().expect("command should run")
}

fn assert_skill_outputs(
    outputs: &[Value],
    expected: &[ExpectedTarget],
    expected_statuses: &[String],
    expected_content: &str,
) {
    assert_eq!(
        outputs.len(),
        expected.len(),
        "expected {} results, got {}",
        expected.len(),
        outputs.len()
    );

    assert_eq!(
        expected_statuses.len(),
        expected.len(),
        "expected statuses must align with expected targets"
    );

    for ((json, expected_target), expected_status) in outputs
        .iter()
        .zip(expected.iter())
        .zip(expected_statuses.iter())
    {
        assert_eq!(json["tool"], expected_target.tool.slug());
        assert_eq!(json["scope"], expected_target.scope.slug());
        assert_eq!(json["status"], expected_status.as_str());
        assert_eq!(
            json["target_path"].as_str(),
            Some(expected_target.path.to_string_lossy().as_ref())
        );

        assert!(
            expected_target.path.is_file(),
            "expected target path to exist: {}",
            expected_target.path.display()
        );

        let written = fs::read_to_string(&expected_target.path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", expected_target.path.display())
        });
        assert_eq!(
            written,
            expected_content,
            "managed bytes should match for {}",
            expected_target.path.display()
        );
    }
}

#[test]
fn skill_final_integration_cli_installs_openai_aligned_managed_skill_for_all_supported_targets() {
    let project = tempdir().expect("project tempdir");
    let home = tempdir().expect("home tempdir");

    let expected = expected_targets(home.path(), project.path());
    assert_eq!(
        expected.len(),
        10,
        "5 tools x 2 scopes must produce 10 targets"
    );

    let managed_content = render_managed_skill_content();
    assert!(
        managed_content.contains("https://developers.openai.com/cookbook/examples/multimodal/image-gen-models-prompting-guide"),
        "managed content must include official OpenAI prompting guide URL"
    );
    assert!(
        managed_content.contains("subject, composition, framing, viewpoint, lighting, and style."),
        "managed content should include guide-aligned prompt-structure language"
    );

    let install_output = run_skill_command(project.path(), home.path(), "install");
    assert!(
        install_output.status.success(),
        "install stderr: {}",
        String::from_utf8_lossy(&install_output.stderr)
    );
    assert!(
        install_output.stderr.is_empty(),
        "install stderr should be empty, got: {}",
        String::from_utf8_lossy(&install_output.stderr)
    );

    let install_payload = parse_json_object(&install_output.stdout);
    assert_eq!(install_payload["operation"], "install");
    let install_expected_statuses = expected_install_statuses(&expected);
    assert_skill_outputs(
        results(&install_payload),
        &expected,
        &install_expected_statuses,
        &managed_content,
    );

    let before_update_bytes: HashMap<PathBuf, String> = expected
        .iter()
        .map(|target| {
            (
                target.path.clone(),
                fs::read_to_string(&target.path).unwrap_or_else(|error| {
                    panic!("failed to snapshot {}: {error}", target.path.display())
                }),
            )
        })
        .collect();

    let update_output = run_skill_command(project.path(), home.path(), "update");
    assert!(
        update_output.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&update_output.stderr)
    );
    assert!(
        update_output.stderr.is_empty(),
        "update stderr should be empty, got: {}",
        String::from_utf8_lossy(&update_output.stderr)
    );

    let update_payload = parse_json_object(&update_output.stdout);
    assert_eq!(update_payload["operation"], "update");
    let update_expected_statuses = expected_unchanged_statuses(&expected);
    assert_skill_outputs(
        results(&update_payload),
        &expected,
        &update_expected_statuses,
        &managed_content,
    );

    for target in &expected {
        let after_update = fs::read_to_string(&target.path)
            .unwrap_or_else(|error| panic!("failed to re-read {}: {error}", target.path.display()));
        let before_update = before_update_bytes
            .get(&target.path)
            .expect("snapshot should contain every target");
        assert_eq!(
            after_update,
            *before_update,
            "update should preserve exact bytes for {}",
            target.path.display()
        );
    }
}

#[test]
fn skill_final_integration_update_blocks_manual_edit_redacts_stderr_and_force_overwrites() {
    let project = tempdir().expect("project tempdir");
    let home = tempdir().expect("home tempdir");
    let target = resolve_skill_path(
        SupportedTool::Pi,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
    let manual_content = "# custom skill\nBearer not-for-output\n";
    fs::write(&target, manual_content).expect("seed manual content");
    let before_bytes = fs::read(&target).expect("manual content should be readable");

    let blocked_output = run_single_target_skill_command(
        project.path(),
        home.path(),
        "update",
        "pi",
        "project",
        false,
    );

    assert_eq!(blocked_output.status.code(), Some(5));
    assert!(blocked_output.stdout.is_empty());

    let blocked_json = parse_single_json_line(&blocked_output.stderr);
    assert_eq!(
        blocked_json["error"]["code"],
        "filesystem.skill_update_blocked_manual_edit"
    );

    let blocked_stderr = String::from_utf8(blocked_output.stderr).expect("stderr should be utf-8");
    assert!(
        !blocked_stderr.contains("Bearer not-for-output"),
        "stderr must redact secret-like marker"
    );

    let preserved_bytes = fs::read(&target).expect("manual content should still be readable");
    assert_eq!(
        preserved_bytes, before_bytes,
        "blocked update should preserve exact manual bytes"
    );

    let forced_output = run_single_target_skill_command(
        project.path(),
        home.path(),
        "update",
        "pi",
        "project",
        true,
    );

    assert!(
        forced_output.status.success(),
        "forced update stderr: {}",
        String::from_utf8_lossy(&forced_output.stderr)
    );
    assert!(forced_output.stderr.is_empty());

    let forced_json = parse_single_json_line(&forced_output.stdout);
    assert_eq!(forced_json["operation"], "update");
    let forced_result = single_result(&forced_json);
    assert_eq!(forced_result["tool"], "pi");
    assert_eq!(forced_result["scope"], "project");
    assert_eq!(forced_result["status"], "forced_overwrite");
    assert_eq!(
        forced_result["target_path"].as_str(),
        Some(target.to_string_lossy().as_ref())
    );

    let overwritten = fs::read_to_string(&target).expect("forced target should be readable");
    assert_eq!(overwritten, render_managed_skill_content());
    assert!(!overwritten.contains("Bearer not-for-output"));
}

#[test]
fn skill_final_integration_install_blocks_tampered_marker_by_default_and_force_overwrites() {
    let project = tempdir().expect("project tempdir");
    let home = tempdir().expect("home tempdir");
    let target = resolve_skill_path(
        SupportedTool::Codex,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
    let tampered_content =
        "<!-- codex-image:managed checksum=deadbeefdeadbeef -->\n# custom skill\nBearer not-for-output\n";
    fs::write(&target, tampered_content).expect("seed tampered content");
    let before_bytes = fs::read(&target).expect("tampered content should be readable");

    let blocked_output = run_single_target_skill_command(
        project.path(),
        home.path(),
        "install",
        "codex",
        "project",
        false,
    );

    assert_eq!(blocked_output.status.code(), Some(5));
    assert!(
        blocked_output.stdout.is_empty(),
        "blocked install must not emit partial success lines"
    );

    let blocked_json = parse_single_json_line(&blocked_output.stderr);
    assert_eq!(
        blocked_json["error"]["code"],
        "filesystem.skill_install_blocked_manual_edit"
    );

    let blocked_stderr = String::from_utf8(blocked_output.stderr).expect("stderr should be utf-8");
    assert!(
        !blocked_stderr.contains("Bearer not-for-output"),
        "stderr must redact secret-like marker"
    );

    let preserved_bytes = fs::read(&target).expect("tampered content should still be readable");
    assert_eq!(
        preserved_bytes, before_bytes,
        "blocked install should preserve exact existing bytes"
    );

    let forced_output = run_single_target_skill_command(
        project.path(),
        home.path(),
        "install",
        "codex",
        "project",
        true,
    );

    assert!(
        forced_output.status.success(),
        "forced install stderr: {}",
        String::from_utf8_lossy(&forced_output.stderr)
    );
    assert!(forced_output.stderr.is_empty());

    let forced_json = parse_single_json_line(&forced_output.stdout);
    assert_eq!(forced_json["operation"], "install");
    let forced_result = single_result(&forced_json);
    assert_eq!(forced_result["tool"], "codex");
    assert_eq!(forced_result["scope"], "project");
    assert_eq!(forced_result["status"], "forced_overwrite");
    assert_eq!(
        forced_result["target_path"].as_str(),
        Some(target.to_string_lossy().as_ref())
    );

    let overwritten = fs::read_to_string(&target).expect("forced target should be readable");
    assert_eq!(overwritten, render_managed_skill_content());
    assert!(!overwritten.contains("Bearer not-for-output"));
}
