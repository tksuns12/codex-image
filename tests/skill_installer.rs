use std::fs;

use codex_image::skill_installer::{
    classify_skill_content, classify_skill_path, install_skill, managed_checksum,
    managed_marker_line, render_managed_skill_content, render_skill_body, uninstall_skill,
    SkillContentClassification, SkillInstallOptions, SkillInstallPlan, SkillInstallStatus,
    SkillUninstallStatus,
};
use codex_image::skills::{resolve_skill_path, SkillScope, SupportedTool};
use tempfile::tempdir;

#[test]
fn skill_installer_content_includes_frontmatter_and_required_headings() {
    let body = render_skill_body();

    assert!(
        body.starts_with("---\nname: codex-image\n"),
        "skill body must start with codex-image frontmatter"
    );
    assert!(
        body.ends_with('\n'),
        "skill body must end with trailing newline"
    );
    assert!(
        body.contains("description:"),
        "skill body must include frontmatter description"
    );

    for heading in [
        "## Command guidance",
        "## Supported tools",
        "## Prompting guide",
        "## Prompt workflow",
        "## Generate checklist",
        "## Edit and preserve checklist",
        "## Text in images",
        "## Multi-image references",
        "## Iteration loop",
        "## Guardrails",
    ] {
        assert!(
            body.contains(heading),
            "skill body must include required heading: {heading}"
        );
    }
}

#[test]
fn skill_installer_content_includes_prompting_checklist_concepts() {
    let body = render_skill_body().to_ascii_lowercase();

    for phrase in [
        "intended use",
        "subject",
        "composition",
        "framing",
        "viewpoint",
        "lighting",
        "style",
        "labeled segments",
        "constraints",
        "invariants",
        "one line",
        "change only the requested element",
        "exact text in double quotes",
        "placement",
        "typography",
        "reference each input by index",
        "controls subject/style/layout",
        "single-change",
        "scoped and reversible",
        "project-controlled directories",
    ] {
        assert!(
            body.contains(phrase),
            "skill body must retain checklist concept phrase: {phrase}"
        );
    }
}

#[test]
fn skill_installer_content_includes_expected_command_guidance() {
    let body = render_skill_body();

    for command in [
        "codex-image generate \"<prompt>\" --out <dir> --output json",
        "codex-image generate --prompt-file ./prompts.txt --out <dir> --output json --timeout 120",
        "codex-image generate --prompt-file ./prompts.txt --out <dir> --output json --debug-diagnostics ./diagnostics.json",
        "codex-image generate \"<prompt>\" --out <dir> --quiet",
    ] {
        assert!(
            body.contains(command),
            "skill body must retain automation command example: {command}"
        );
    }
}

#[test]
fn skill_installer_content_includes_automation_contract_concepts() {
    let body = render_skill_body().to_ascii_lowercase();

    for phrase in [
        "--output json",
        "--quiet",
        "--prompt-file",
        "--timeout",
        "--debug-diagnostics",
        "item-0001",
        "blank lines and lines beginning with `#` are skipped",
        "root `manifest.json` is written only after every item succeeds",
        "local codex subprocess timeout",
        "diagnostics are sanitized/redacted",
        "not a raw codex transcript",
        "when `--quiet` is used, rely on generated files and exit status",
    ] {
        assert!(
            body.contains(phrase),
            "skill body must retain automation contract phrase: {phrase}"
        );
    }
}

#[test]
fn skill_installer_content_lists_all_supported_tool_slugs() {
    let body = render_skill_body();

    for tool in SupportedTool::all() {
        assert!(
            body.contains(tool.slug()),
            "skill body must mention supported tool slug: {}",
            tool.slug()
        );
    }
}

#[test]
fn skill_installer_content_contains_prompting_guide_url() {
    let body = render_skill_body();
    let guide_url =
        "https://developers.openai.com/cookbook/examples/multimodal/image-gen-models-prompting-guide";

    assert!(
        body.contains(guide_url),
        "skill body must retain official OpenAI prompting guide URL: {guide_url}"
    );
}

#[test]
fn skill_installer_content_excludes_banned_auth_and_api_strings() {
    let managed = render_managed_skill_content();
    let managed_lower = managed.to_ascii_lowercase();
    let guide_url =
        "https://developers.openai.com/cookbook/examples/multimodal/image-gen-models-prompting-guide";

    assert!(
        managed.contains(guide_url),
        "managed skill content must keep official guide URL even while enforcing banned-token checks"
    );

    for banned in [
        "OPENAI_API_KEY",
        "CODEX_IMAGE_API_BASE_URL",
        "CODEX_IMAGE_AUTH_BASE_URL",
        "codex-image login",
        "oauth",
        "api key",
        "Bearer ",
        "curl",
        "python",
        "Image API",
        "Responses API",
    ] {
        assert!(
            !managed_lower.contains(&banned.to_ascii_lowercase()),
            "managed skill content must not include banned token: {banned}"
        );
    }
}

#[test]
fn skill_installer_content_is_deterministic_bytes_and_checksum() {
    let first = render_managed_skill_content();
    let second = render_managed_skill_content();

    assert_eq!(
        first.as_bytes(),
        second.as_bytes(),
        "managed render must be byte-stable across calls"
    );

    let body = render_skill_body();
    let expected_marker = managed_marker_line(body);

    let first_byte = first
        .as_bytes()
        .first()
        .copied()
        .expect("managed content must not be empty");
    assert_eq!(
        first_byte, b'-',
        "managed content byte 1 must begin frontmatter markdown"
    );

    let first_line = first.lines().next().expect("first line exists");
    assert_eq!(
        first_line, "---",
        "managed skill must start with frontmatter"
    );
    assert!(
        first.ends_with('\n'),
        "managed skill content must end with a trailing newline"
    );

    let marker_lines: Vec<_> = first
        .lines()
        .filter(|line| line.starts_with("<!-- codex-image:managed checksum="))
        .collect();
    assert_eq!(
        marker_lines.len(),
        1,
        "managed content must include exactly one checksum marker"
    );

    let actual_marker = first
        .lines()
        .last()
        .expect("marker line exists at end of file");
    assert_eq!(
        actual_marker, expected_marker,
        "managed checksum marker must be final line"
    );

    let marker_checksum = actual_marker
        .strip_prefix("<!-- codex-image:managed checksum=")
        .and_then(|value| value.strip_suffix(" -->"))
        .expect("marker must use stable checksum shape");
    assert_eq!(
        marker_checksum.len(),
        16,
        "checksum marker must contain 16 hex digits"
    );
    assert!(
        marker_checksum.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "checksum marker must contain only ASCII hex digits"
    );
    assert_eq!(
        marker_checksum,
        managed_checksum(body),
        "marker checksum must equal managed_checksum(render_skill_body())"
    );
}

#[test]
fn skill_installer_content_treats_legacy_marker_first_layout_as_managed_outdated() {
    let body = render_skill_body();
    let legacy_layout = format!("{}\n{}", managed_marker_line(body), body);

    assert_eq!(
        classify_skill_content(Some(&legacy_layout)),
        SkillContentClassification::ManagedOutdated
    );
}

#[test]
fn skill_installer_content_classifies_missing_manual_outdated_current_and_tampered() {
    assert_eq!(
        classify_skill_content(None),
        SkillContentClassification::Missing
    );

    assert_eq!(
        classify_skill_content(Some("# manual skill\ncontent\n")),
        SkillContentClassification::ManualUnmanaged
    );

    let current = render_managed_skill_content();
    assert_eq!(
        classify_skill_content(Some(&current)),
        SkillContentClassification::ManagedCurrent
    );

    let outdated_body = render_skill_body().replace(
        "Keep outputs in project-controlled directories.",
        "Keep outputs in checked-in workspace directories.",
    );
    let outdated = format!("{}\n{}", managed_marker_line(&outdated_body), outdated_body);
    assert_eq!(
        classify_skill_content(Some(&outdated)),
        SkillContentClassification::ManagedOutdated
    );

    let tampered = current.replacen(
        "Keep outputs in project-controlled directories.",
        "Keep outputs anywhere.",
        1,
    );
    assert_eq!(
        classify_skill_content(Some(&tampered)),
        SkillContentClassification::ManagedTampered
    );
}

#[test]
fn skill_installer_content_classifies_malformed_managed_metadata_as_tampered() {
    let body = render_skill_body();
    let malformed_marker = "<!-- codex-image:managed checksum=not-hex -->";
    let malformed = format!("{malformed_marker}\n{body}");
    assert_eq!(
        classify_skill_content(Some(&malformed)),
        SkillContentClassification::ManagedTampered
    );
}

#[test]
fn skill_installer_filesystem_classify_path_reports_missing_and_manual() {
    let workspace = tempdir().expect("workspace tempdir");
    let missing = workspace.path().join("missing").join("SKILL.md");

    assert_eq!(
        classify_skill_path(&missing).expect("missing should classify"),
        SkillContentClassification::Missing
    );

    let manual = workspace.path().join("manual").join("SKILL.md");
    fs::create_dir_all(manual.parent().expect("manual parent")).expect("create parent");
    fs::write(&manual, "# custom skill\nnotes\n").expect("write manual file");

    assert_eq!(
        classify_skill_path(&manual).expect("manual should classify"),
        SkillContentClassification::ManualUnmanaged
    );
}

#[test]
fn skill_installer_filesystem_blocks_malformed_checksum_marker_by_default() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::Claude,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    let malformed = format!(
        "<!-- codex-image:managed checksum=zzzz -->\n{}",
        render_skill_body()
    );

    let parent = plan.target_path().parent().expect("parent directory");
    fs::create_dir_all(parent).expect("create parent");
    fs::write(plan.target_path(), &malformed).expect("seed malformed managed metadata");

    let result = install_skill(&plan, SkillInstallOptions::default()).expect("install succeeds");
    assert_eq!(result.status, SkillInstallStatus::BlockedManualEdit);

    let preserved = fs::read_to_string(plan.target_path()).expect("malformed content preserved");
    assert_eq!(preserved, malformed);
}

#[test]
fn skill_installer_filesystem_plan_build_matches_resolved_path() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");

    let plan = SkillInstallPlan::build(
        SupportedTool::OpenCode,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    assert_eq!(
        plan.target_path(),
        resolve_skill_path(
            SupportedTool::OpenCode,
            SkillScope::ProjectLocal,
            home.path(),
            project.path()
        )
    );
}

#[test]
fn skill_installer_filesystem_creates_missing_skill_file() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");

    let plan = SkillInstallPlan::build(
        SupportedTool::Pi,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    assert!(!plan.target_path().exists());

    let result = install_skill(&plan, SkillInstallOptions::default()).expect("install succeeds");
    assert_eq!(result.status, SkillInstallStatus::Created);
    assert_eq!(result.path, plan.target_path());

    let written = fs::read_to_string(plan.target_path()).expect("managed file written");
    assert_eq!(written, render_managed_skill_content());
}

#[test]
fn skill_installer_filesystem_noops_when_already_managed_current() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::Claude,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    let first = install_skill(&plan, SkillInstallOptions::default()).expect("first install");
    assert_eq!(first.status, SkillInstallStatus::Created);

    let second = install_skill(&plan, SkillInstallOptions::default()).expect("second install");
    assert_eq!(second.status, SkillInstallStatus::Unchanged);
}

#[test]
fn skill_installer_filesystem_updates_valid_managed_outdated_content() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::ClaudeCode,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    let outdated_body = render_skill_body().replace(
        "Keep outputs in project-controlled directories.",
        "Keep outputs in checked-in workspace directories.",
    );
    let outdated = format!("{}\n{}", managed_marker_line(&outdated_body), outdated_body);

    let parent = plan.target_path().parent().expect("parent directory");
    fs::create_dir_all(parent).expect("create parent");
    fs::write(plan.target_path(), outdated).expect("seed outdated managed content");

    let result = install_skill(&plan, SkillInstallOptions::default()).expect("update succeeds");
    assert_eq!(result.status, SkillInstallStatus::Updated);

    let rewritten = fs::read_to_string(plan.target_path()).expect("managed content rewritten");
    assert_eq!(rewritten, render_managed_skill_content());
}

#[test]
fn skill_installer_filesystem_blocks_unmanaged_manual_edits_by_default() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::Codex,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    let parent = plan.target_path().parent().expect("parent directory");
    fs::create_dir_all(parent).expect("create parent");
    let manual = "# custom skill\nnotes: preserve manual edits\napi_key=local-dev-only\n";
    fs::write(plan.target_path(), manual).expect("seed manual skill");

    let result = install_skill(&plan, SkillInstallOptions::default()).expect("install succeeds");
    assert_eq!(result.status, SkillInstallStatus::BlockedManualEdit);

    let current = fs::read_to_string(plan.target_path()).expect("manual content preserved");
    assert_eq!(current, manual);
}

#[test]
fn skill_installer_filesystem_blocks_tampered_managed_edits_by_default() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::OpenCode,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    let tampered = render_managed_skill_content().replacen(
        "Keep outputs in project-controlled directories.",
        "Keep outputs in personal desktop directories.",
        1,
    );

    let parent = plan.target_path().parent().expect("parent directory");
    fs::create_dir_all(parent).expect("create parent");
    fs::write(plan.target_path(), &tampered).expect("seed tampered managed content");

    let result = install_skill(&plan, SkillInstallOptions::default()).expect("install succeeds");
    assert_eq!(result.status, SkillInstallStatus::BlockedManualEdit);

    let current = fs::read_to_string(plan.target_path()).expect("tampered content preserved");
    assert_eq!(current, tampered);
}

#[test]
fn skill_installer_filesystem_force_overwrites_blocked_manual_or_tampered_content() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::Pi,
        SkillScope::Global,
        home.path(),
        project.path(),
    );

    let parent = plan.target_path().parent().expect("parent directory");
    fs::create_dir_all(parent).expect("create parent");
    fs::write(plan.target_path(), "# my custom global skill\n").expect("seed manual content");

    let result = install_skill(&plan, SkillInstallOptions { force: true }).expect("forced install");
    assert_eq!(result.status, SkillInstallStatus::ForcedOverwrite);

    let current = fs::read_to_string(plan.target_path()).expect("forced rewrite");
    assert_eq!(current, render_managed_skill_content());
}

#[test]
fn skill_installer_filesystem_uninstall_deletes_managed_and_missing_is_noop() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::Pi,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    let missing =
        uninstall_skill(&plan, SkillInstallOptions::default()).expect("missing uninstall");
    assert_eq!(missing.status, SkillUninstallStatus::AlreadyMissing);

    let created = install_skill(&plan, SkillInstallOptions::default()).expect("install managed");
    assert_eq!(created.status, SkillInstallStatus::Created);
    assert!(plan.target_path().is_file());

    let deleted = uninstall_skill(&plan, SkillInstallOptions::default()).expect("delete managed");
    assert_eq!(deleted.status, SkillUninstallStatus::Deleted);
    assert!(!plan.target_path().exists());
}

#[test]
fn skill_installer_filesystem_uninstall_blocks_manual_or_tampered_without_force() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::OpenCode,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    let parent = plan.target_path().parent().expect("parent directory");
    fs::create_dir_all(parent).expect("create parent");

    fs::write(plan.target_path(), "# manual skill\n").expect("seed manual skill");
    let blocked_manual =
        uninstall_skill(&plan, SkillInstallOptions::default()).expect("manual blocked");
    assert_eq!(
        blocked_manual.status,
        SkillUninstallStatus::BlockedManualEdit
    );
    assert!(plan.target_path().exists());

    let tampered = render_managed_skill_content().replacen(
        "Keep outputs in project-controlled directories.",
        "Keep outputs in home desktop directories.",
        1,
    );
    fs::write(plan.target_path(), tampered).expect("seed tampered managed skill");
    let blocked_tampered =
        uninstall_skill(&plan, SkillInstallOptions::default()).expect("tampered blocked");
    assert_eq!(
        blocked_tampered.status,
        SkillUninstallStatus::BlockedManualEdit
    );
    assert!(plan.target_path().exists());
}

#[test]
fn skill_installer_filesystem_uninstall_force_deletes_protected_content() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");
    let plan = SkillInstallPlan::build(
        SupportedTool::Codex,
        SkillScope::Global,
        home.path(),
        project.path(),
    );

    let parent = plan.target_path().parent().expect("parent directory");
    fs::create_dir_all(parent).expect("create parent");
    fs::write(plan.target_path(), "# manual skill\n").expect("seed manual content");

    let result =
        uninstall_skill(&plan, SkillInstallOptions { force: true }).expect("forced delete");
    assert_eq!(result.status, SkillUninstallStatus::ForcedDelete);
    assert!(!plan.target_path().exists());
}

#[test]
fn skill_installer_filesystem_codex_and_pi_duplicate_path_is_idempotent() {
    let home = tempdir().expect("home tempdir");
    let project = tempdir().expect("project tempdir");

    let codex_plan = SkillInstallPlan::build(
        SupportedTool::Codex,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );
    let pi_plan = SkillInstallPlan::build(
        SupportedTool::Pi,
        SkillScope::ProjectLocal,
        home.path(),
        project.path(),
    );

    assert_eq!(codex_plan.target_path(), pi_plan.target_path());

    let first = install_skill(&codex_plan, SkillInstallOptions::default()).expect("first install");
    assert_eq!(first.status, SkillInstallStatus::Created);

    let second = install_skill(&pi_plan, SkillInstallOptions::default()).expect("second install");
    assert_eq!(second.status, SkillInstallStatus::Unchanged);
}
