use codex_image::updater::{release_asset_name_for_tag, Platform};

#[test]
fn release_workflow_is_scoped_to_release_branch_and_release_please() {
    let workflow = include_str!("../.github/workflows/release.yml");

    assert!(
        workflow.contains("pull_request:\n    branches:\n      - release"),
        "release workflow must run preflight on pull requests into release"
    );
    assert!(
        workflow.contains("push:\n    branches:\n      - release"),
        "release workflow must publish only from pushes to release"
    );
    assert!(
        workflow.contains("googleapis/release-please-action@v4"),
        "release workflow must delegate semver/release PRs to release-please"
    );
    assert!(
        workflow.contains("if: ${{ github.event_name == 'push' }}"),
        "release-please must not run on pull request preflight checks"
    );
    assert!(
        workflow.contains("contents: read")
            && workflow.contains("contents: write")
            && workflow.contains("pull-requests: write"),
        "release workflow permissions must default to read and elevate only release jobs"
    );
    assert!(
        workflow.contains("release-type: rust"),
        "release-please must use Rust/Cargo versioning"
    );
    assert!(
        workflow.contains("target-branch: release"),
        "release-please must target the release branch"
    );
    assert!(
        !workflow.contains("package-name:"),
        "release-please-action v4 no longer accepts package-name input"
    );
    assert!(
        include_str!("../CHANGELOG.md").contains("release-please"),
        "release-please Rust releases need a changelog seed file"
    );
    assert!(
        workflow.contains("dtolnay/rust-toolchain@stable"),
        "release workflow must install Rust explicitly for GitHub and act runners"
    );
    assert!(
        workflow.contains("cargo test --locked"),
        "release workflow must run tests before release-please"
    );
    assert!(
        workflow.contains("cargo clippy --locked --all-targets --all-features -- -D warnings"),
        "release workflow must run clippy before release-please"
    );
}

#[test]
fn release_workflow_builds_expected_platform_artifacts() {
    let workflow = include_str!("../.github/workflows/release.yml");

    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(
            workflow.contains(target),
            "release workflow must build target {target}"
        );
    }

    assert!(
        workflow.contains("cargo build --locked --release --target"),
        "release workflow must build locked release binaries"
    );
    assert!(
        workflow.contains("gh release upload"),
        "release workflow must upload archives to the GitHub Release"
    );
    assert!(
        workflow.contains("--clobber"),
        "release workflow reruns should replace existing failed-run artifacts"
    );
    assert!(
        !workflow.contains("Compress-Archive -Path \"$packageDir/*\""),
        "windows archive packaging must not flatten the package root via $packageDir/*"
    );
    assert!(
        workflow.contains("Push-Location dist")
            && workflow.contains("Compress-Archive -Path $asset -DestinationPath \"$asset.zip\"")
            && workflow.contains("Pop-Location"),
        "windows archive packaging must preserve the top-level $asset directory in the zip"
    );
}

#[test]
fn release_workflow_publishes_aggregate_sha256sums() {
    let workflow = include_str!("../.github/workflows/release.yml");

    assert!(
        workflow.contains("actions/upload-artifact@v4"),
        "matrix builds must upload archives as workflow artifacts for aggregation"
    );
    assert!(
        workflow.contains("actions/download-artifact@v4"),
        "publish job must download all matrix artifacts before checksumming"
    );
    assert!(
        workflow.contains("pattern: release-archive-*")
            && workflow.contains("merge-multiple: true"),
        "publish job must merge release archive artifacts into one directory"
    );
    assert!(
        workflow.contains("name: Publish release assets")
            && workflow.contains("needs: [release-please, build-artifacts]"),
        "release assets must be published only after every matrix archive is built"
    );
    assert!(
        workflow.contains("Generate aggregate SHA256SUMS")
            && workflow.contains("sha256sum \"${archives[@]}\" > SHA256SUMS"),
        "publish job must compute one aggregate SHA256SUMS file from all archives"
    );
    assert!(
        workflow.contains("LC_ALL=C sort"),
        "SHA256SUMS generation must be deterministic across reruns"
    );
    assert!(
        workflow
            .contains("gh release upload \"$RELEASE_TAG\" \"${archives[@]}\" SHA256SUMS --clobber"),
        "publish job must upload archives and SHA256SUMS to the GitHub Release with --clobber"
    );
}

#[test]
fn release_workflow_targets_have_updater_mapping_and_naming_contract() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let workflow_targets = workflow_matrix_targets(workflow);

    assert_eq!(
        workflow_targets,
        vec![
            "x86_64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ],
        "matrix target list changed; update updater mapping and this contract test together"
    );

    for target in workflow_targets {
        let platform = workflow_target_to_platform(target).unwrap_or_else(|reason| {
            panic!("workflow target {target} has no updater mapping: {reason}")
        });

        assert_eq!(
            platform.rust_target(),
            target,
            "updater rust target must match workflow target exactly"
        );

        let asset = release_asset_name_for_tag("v9.9.9", &platform);
        let expected_extension = if target.ends_with("-pc-windows-msvc") {
            ".zip"
        } else {
            ".tar.gz"
        };

        assert!(
            asset == format!("codex-image-v9.9.9-{target}{expected_extension}"),
            "updater asset naming drifted for {target}: {asset}"
        );
    }
}

#[test]
fn unsupported_future_release_target_is_explicit_contract_break() {
    let err = workflow_target_to_platform("aarch64-unknown-linux-gnu")
        .expect_err("unsupported release target must fail until updater mapping is added");

    assert!(
        err.contains("unsupported updater platform"),
        "unexpected error message: {err}"
    );
}

fn workflow_matrix_targets(workflow: &str) -> Vec<&str> {
    workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .collect()
}

fn workflow_target_to_platform(target: &str) -> Result<Platform, String> {
    let (os, arch) = parse_target_os_arch(target)
        .ok_or_else(|| format!("target triple format not recognized: {target}"))?;

    Platform::new(os, arch)
        .map_err(|_| format!("unsupported updater platform {os}/{arch} for target {target}"))
}

fn parse_target_os_arch(target: &str) -> Option<(&'static str, &'static str)> {
    match target {
        "x86_64-unknown-linux-gnu" => Some(("linux", "x86_64")),
        "x86_64-apple-darwin" => Some(("macos", "x86_64")),
        "aarch64-apple-darwin" => Some(("macos", "aarch64")),
        "x86_64-pc-windows-msvc" => Some(("windows", "x86_64")),
        "aarch64-unknown-linux-gnu" => Some(("linux", "aarch64")),
        _ => None,
    }
}
