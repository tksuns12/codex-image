fn must_contain(label: &str, doc: &str, marker: &str) -> usize {
    doc.find(marker)
        .unwrap_or_else(|| panic!("{label} must include marker: {marker}"))
}

#[allow(dead_code)]
fn assert_before(label: &str, doc: &str, first: &str, second: &str) {
    let first_pos = must_contain(label, doc, first);
    let second_pos = must_contain(label, doc, second);
    assert!(
        first_pos < second_pos,
        "{label} must place '{first}' before '{second}' (positions: {first_pos} vs {second_pos})"
    );
}

fn assert_all_present(label: &str, doc: &str, markers: &[&str]) -> Vec<usize> {
    markers
        .iter()
        .map(|marker| must_contain(label, doc, marker))
        .collect()
}

fn assert_all_absent(label: &str, doc: &str, banned: &[&str]) {
    for marker in banned {
        assert!(
            !doc.contains(marker),
            "{label} must not include removed/internal marker: {marker}"
        );
    }
}

fn assert_secondary_markers_after_first_success(
    label: &str,
    doc: &str,
    first_success_markers: &[&str],
    secondary_markers: &[&str],
) {
    for first_success_marker in first_success_markers {
        for secondary_marker in secondary_markers {
            assert_before(label, doc, first_success_marker, secondary_marker);
        }
    }
}

#[test]
fn install_uat_docs_readme_covers_install_usage_and_codex_backend() {
    let label = "README";
    let readme = include_str!("../README.md");

    assert_all_present(
        label,
        readme,
        &[
            "cargo install --path .",
            "raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.sh",
            "raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.ps1",
            "curl -fsSL https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.sh | sh",
            "Invoke-RestMethod https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.ps1 | Invoke-Expression",
            "CODEX_IMAGE_INSTALL_DIR",
            "SHA256SUMS",
            "codex-image",
            "generate",
            "CODEX_IMAGE_CODEX_BIN",
            "README.ko.md",
            "macOS-only",
            "docs/advanced-reference.md",
            "docs/skill-paths.md",
            "docs/uat-live-smoke.md",
        ],
    );

    assert!(
        !readme.contains("VERSION=\"v0.1.0\"") && !readme.contains("$Version = \"v0.1.0\""),
        "{label} install snippets must resolve the latest release instead of pinning v0.1.0"
    );

    assert!(
        readme.contains("Codex CLI") || readme.contains("Codex installation"),
        "{label} must document the Codex dependency"
    );
    assert!(
        readme.contains("Codex extensions")
            || (readme.contains("VS Code") && readme.contains("Cursor")),
        "{label} must mention Codex extension install locations as a prerequisite"
    );

    assert_all_present(
        label,
        readme,
        &[
            "`codex-image` is a small CLI",
            "## Prerequisite: Codex CLI / Codex extensions",
            "macOS-only",
            "VS Code/Cursor",
            "Codex must already be logged in and able to use its built-in image generation tool.",
            "## Install",
            "verifies the downloaded archive against `SHA256SUMS`",
            "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
            "image-0001.<format>",
            "manifest.json",
            "human-readable success line on stdout",
            "manifest.json` is always written under `--out`",
            "codex-image generate \"A watercolor fox reading in a library\" --out ./out --output json",
            "--output json",
            "--quiet",
            "--prompt-file",
            "--timeout",
            "--debug-diagnostics",
            "## Post-first-run references (optional)",
            "codex-image skill install --tool",
            "codex-image skill update --tool",
            "codex-image update --dry-run --output json",
            "codex-image update\n",
            "--version v1.2.3",
            "docs/advanced-reference.md",
            "docs/skill-paths.md",
            "docs/uat-live-smoke.md",
        ],
    );

    assert_before(
        label,
        readme,
        "`codex-image` is a small CLI",
        "## Prerequisite: Codex CLI / Codex extensions",
    );
    assert_before(
        label,
        readme,
        "## Prerequisite: Codex CLI / Codex extensions",
        "## Install",
    );
    assert_before(
        label,
        readme,
        "macOS-only",
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
    );
    assert_before(
        label,
        readme,
        "VS Code/Cursor",
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
    );
    assert_before(
        label,
        readme,
        "Codex must already be logged in and able to use its built-in image generation tool.",
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
    );
    assert_before(
        label,
        readme,
        "CODEX_IMAGE_CODEX_BIN",
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
    );

    assert_before(
        label,
        readme,
        "## Install",
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
    );
    assert_before(
        label,
        readme,
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
        "image-0001.<format>",
    );
    assert_before(
        label,
        readme,
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
        "manifest.json",
    );
    assert_before(
        label,
        readme,
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out",
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out --output json",
    );

    let secondary_markers = [
        "## Post-first-run references (optional)",
        "docs/advanced-reference.md",
        "docs/skill-paths.md",
        "codex-image generate \"A watercolor fox reading in a library\" --out ./out --quiet",
        "--prompt-file ./prompt.txt",
        "--timeout 120",
        "--debug-diagnostics",
        "codex-image skill install --tool",
        "codex-image skill update --tool",
        "codex-image update --dry-run --output json",
        "codex-image update\n",
        "--version v1.2.3",
        "docs/uat-live-smoke.md",
    ];

    assert_secondary_markers_after_first_success(
        label,
        readme,
        &["image-0001.<format>", "manifest.json"],
        &secondary_markers,
    );

    assert_all_absent(
        label,
        readme,
        &[
            "## Agent skill install/update guide",
            "## Verification scripts",
            "actions/workflows/release.yml/badge.svg?branch=release",
            "release-please",
            "Release / Preflight",
            "CODEX_IMAGE_API_BASE_URL",
            "CODEX_IMAGE_AUTH_BASE_URL",
            "status --json",
            "codex-image login",
        ],
    );
}

#[test]
fn install_uat_docs_advanced_reference_covers_post_first_run_operations() {
    let label = "Advanced reference";
    let advanced_reference = include_str!("../docs/advanced-reference.md");

    assert_all_present(
        label,
        advanced_reference,
        &[
            "Space",
            "Enter",
            "installed:outdated",
            "installed:protected",
            "--force",
            "codex-image skill install --tool codex --tool pi --scope project --yes",
            "codex-image skill update --tool codex --scope project --yes",
            "line-delimited JSON rows",
            "tool",
            "scope",
            "status",
            "target_path",
            "Do not mutate authentication state",
            "codex-image update --dry-run",
            "codex-image update\n",
            "codex-image update --version v1.2.3",
            "GitHub Release artifacts",
            "Windows same-process replacement limitation",
            "no live GitHub downloads",
            "no live Codex generation",
            "no credentials",
            "no auth mutation",
            "scripts/uat-live-smoke.sh",
            "scripts/verify-local-install.sh",
            "docs/skill-paths.md",
            "docs/uat-live-smoke.md",
            "https://developers.openai.com/cookbook/examples/multimodal/image-gen-models-prompting-guide",
        ],
    );

    assert_all_absent(
        label,
        advanced_reference,
        &[
            "actions/workflows/release.yml/badge.svg?branch=release",
            "release-please",
            "Release / Preflight",
            "CODEX_IMAGE_API_BASE_URL",
            "CODEX_IMAGE_AUTH_BASE_URL",
            "status --json",
            "codex-image login",
        ],
    );
}

#[test]
fn install_uat_docs_korean_readme_covers_install_usage_and_codex_backend() {
    let label = "Korean README";
    let readme = include_str!("../README.ko.md");

    assert_all_present(
        label,
        readme,
        &[
            "cargo install --path .",
            "raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.sh",
            "raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.ps1",
            "curl -fsSL https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.sh | sh",
            "Invoke-RestMethod https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.ps1 | Invoke-Expression",
            "CODEX_IMAGE_INSTALL_DIR",
            "codex-image",
            "generate",
            "CODEX_IMAGE_CODEX_BIN",
            "README.md",
            "docs/advanced-reference.md",
            "docs/skill-paths.md",
            "docs/uat-live-smoke.md",
        ],
    );

    assert!(
        !readme.contains("VERSION=\"v0.1.0\"") && !readme.contains("$Version = \"v0.1.0\""),
        "{label} install snippets must resolve the latest release instead of pinning v0.1.0"
    );

    assert!(
        readme.contains("Codex CLI") || readme.contains("Codex 설치"),
        "{label} must document the Codex dependency"
    );
    assert!(
        readme.contains("Codex 확장") || (readme.contains("VS Code") && readme.contains("Cursor")),
        "{label} must mention Codex extension install locations as a prerequisite"
    );
    assert!(
        readme.contains("macOS 전용") || readme.contains("macOS"),
        "{label} must state that standalone Codex CLI support is macOS-only"
    );

    assert_all_present(
        label,
        readme,
        &[
            "`codex-image`는 설치된 Codex CLI에 이미지 생성을 맡기는 작은 CLI입니다.",
            "## 사전 요구 사항: Codex CLI / Codex 확장",
            "standalone Codex CLI는 현재 **macOS 전용**입니다.",
            "Codex는 이미 로그인되어 있어야 하며, 내장 이미지 생성 도구를 사용할 수 있어야 합니다.",
            "## 설치",
            "## 이미지와 매니페스트 생성",
            "codex-image generate \"도서관에서 책을 읽는 수채화풍 여우\" --out ./out",
            "image-0001.<format>",
            "manifest.json",
            "## 첫 실행 후",
            "## 첫 실행 이후 참고 자료 (선택)",
            "codex-image skill install --tool",
            "codex-image skill update --tool",
            "codex-image update --dry-run",
            "codex-image update\n",
            "--version v1.2.3",
            "docs/advanced-reference.md",
            "docs/skill-paths.md",
            "docs/uat-live-smoke.md",
        ],
    );

    assert_before(
        label,
        readme,
        "`codex-image`는 설치된 Codex CLI에 이미지 생성을 맡기는 작은 CLI입니다.",
        "## 사전 요구 사항: Codex CLI / Codex 확장",
    );
    assert_before(
        label,
        readme,
        "## 사전 요구 사항: Codex CLI / Codex 확장",
        "## 설치",
    );
    assert_before(label, readme, "## 설치", "## 이미지와 매니페스트 생성");
    assert_before(
        label,
        readme,
        "## 이미지와 매니페스트 생성",
        "## 첫 실행 후",
    );
    assert_before(
        label,
        readme,
        "## 첫 실행 후",
        "## 첫 실행 이후 참고 자료 (선택)",
    );
    assert_before(
        label,
        readme,
        "standalone Codex CLI는 현재 **macOS 전용**입니다.",
        "codex-image generate \"도서관에서 책을 읽는 수채화풍 여우\" --out ./out",
    );
    assert_before(
        label,
        readme,
        "Codex는 이미 로그인되어 있어야 하며, 내장 이미지 생성 도구를 사용할 수 있어야 합니다.",
        "codex-image generate \"도서관에서 책을 읽는 수채화풍 여우\" --out ./out",
    );
    assert_before(
        label,
        readme,
        "CODEX_IMAGE_CODEX_BIN",
        "codex-image generate \"도서관에서 책을 읽는 수채화풍 여우\" --out ./out",
    );
    assert_before(
        label,
        readme,
        "codex-image generate \"도서관에서 책을 읽는 수채화풍 여우\" --out ./out",
        "image-0001.<format>",
    );
    assert_before(
        label,
        readme,
        "codex-image generate \"도서관에서 책을 읽는 수채화풍 여우\" --out ./out",
        "manifest.json",
    );

    let secondary_markers = [
        "## 첫 실행 이후 참고 자료 (선택)",
        "docs/advanced-reference.md",
        "docs/skill-paths.md",
        "codex-image skill install --tool",
        "codex-image skill update --tool",
        "codex-image update --dry-run",
        "codex-image update\n",
        "--version v1.2.3",
        "docs/uat-live-smoke.md",
    ];

    assert_secondary_markers_after_first_success(
        label,
        readme,
        &["image-0001.<format>", "manifest.json"],
        &secondary_markers,
    );

    assert_all_absent(
        label,
        readme,
        &[
            "## 에이전트 스킬 설치/업데이트 가이드",
            "## 환경 변수",
            "## 검증 스크립트",
            "Space",
            "Enter",
            "no live GitHub downloads",
            "no live Codex generation",
            "no credentials",
            "no auth mutation",
            "actions/workflows/release.yml/badge.svg?branch=release",
            "release-please",
            "Release / Preflight",
            "CODEX_IMAGE_API_BASE_URL",
            "CODEX_IMAGE_AUTH_BASE_URL",
            "status --json",
            "codex-image login",
        ],
    );
}

#[test]
fn install_uat_docs_scripts_document_codex_backend_and_live_guard() {
    let install_label = "install-latest script";
    let install_ps1_label = "install-latest PowerShell script";
    let verify_label = "verify-local-install script";
    let live_label = "live UAT script";

    let install_latest = include_str!("../scripts/install-latest.sh");
    let install_latest_ps1 = include_str!("../scripts/install-latest.ps1");
    let verify_local_install = include_str!("../scripts/verify-local-install.sh");
    let uat_live_smoke = include_str!("../scripts/uat-live-smoke.sh");

    assert_all_present(
        install_label,
        install_latest,
        &[
            "#!/usr/bin/env sh",
            "tksuns12/codex-image",
            "https://api.github.com/repos/${REPO}/releases/latest",
            "could not resolve latest codex-image release",
            "x86_64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "releases/download/${VERSION}/${ASSET}",
            "CODEX_IMAGE_INSTALL_DIR",
            "--help >/dev/null",
        ],
    );
    assert_all_present(
        install_ps1_label,
        install_latest_ps1,
        &[
            "tksuns12/codex-image",
            "Invoke-RestMethod $ApiUrl",
            "could not resolve latest codex-image release",
            "x86_64-pc-windows-msvc",
            "releases/download/$Version/$Asset",
            "CODEX_IMAGE_INSTALL_DIR",
            "codex-image.exe",
            "--help",
        ],
    );

    assert_all_present(
        verify_label,
        verify_local_install,
        &[
            "generate --help",
            "'--out'",
            "'--output'",
            "'--quiet'",
            "'--prompt-file'",
            "'--timeout'",
            "'--debug-diagnostics'",
            "status --json",
            "login --help",
            "logout --help",
        ],
    );

    let live_generate_command =
        "generate \"UAT smoke image from codex-image\" --out \"$out_dir\" --output json";
    assert_all_present(
        live_label,
        uat_live_smoke,
        &[
            "CODEX_IMAGE_RUN_LIVE=1",
            "CODEX_IMAGE_CODEX_BIN",
            live_generate_command,
            "json.load(fh)",
            "manifest_path",
            "cli-stderr-redacted",
        ],
    );
    assert_before(live_label, uat_live_smoke, live_generate_command, "json.load(fh)");

    assert_all_absent(
        live_label,
        uat_live_smoke,
        &[
            "CODEX_IMAGE_API_BASE_URL",
            "CODEX_IMAGE_AUTH_BASE_URL",
            "status --json",
            " login",
        ],
    );
}

#[test]
fn install_uat_docs_uat_doc_describes_codex_only_backend() {
    let label = "UAT runbook";
    let runbook = include_str!("../docs/uat-live-smoke.md");

    assert_all_present(
        label,
        runbook,
        &[
            "Codex",
            "generate",
            "CODEX_IMAGE_CODEX_BIN",
            "CODEX_IMAGE_RUN_LIVE=1 bash scripts/uat-live-smoke.sh",
            "generate \"UAT smoke image from codex-image\" --out <temp-out> --output json",
            "Parses stdout JSON (explicitly emitted by `--output json`) and validates",
            "manifest_path",
            "phase=generate",
            "redacted CLI stderr envelope",
        ],
    );

    assert_all_absent(
        label,
        runbook,
        &[
            "CODEX_IMAGE_AUTH_BASE_URL",
            "CODEX_IMAGE_API_BASE_URL",
            "status --json",
            "OAuth",
            "CODEX_IMAGE_RUN_LIVE=0",
        ],
    );
}
