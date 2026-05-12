fn assert_contains(haystack: &str, needle: &str, context: &str) {
    assert!(
        haystack.contains(needle),
        "{context}: expected script to contain {needle:?}"
    );
}

fn assert_sequence(haystack: &str, sequence: &[&str], context: &str) {
    let mut cursor = 0;

    for needle in sequence {
        let relative_index = haystack[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("{context}: missing ordered marker {needle:?}"));
        cursor += relative_index + needle.len();
    }
}

#[test]
fn shell_installer_downloads_sha256sums_and_selects_a_verifier() {
    let script = include_str!("../scripts/install-latest.sh");

    assert_contains(
        script,
        "/releases/download/${VERSION}/SHA256SUMS",
        "shell installer must download release checksum metadata",
    );
    assert_contains(
        script,
        "command -v sha256sum",
        "shell installer must prefer sha256sum when available",
    );
    assert_contains(
        script,
        "command -v shasum",
        "shell installer must fall back to shasum when sha256sum is unavailable",
    );
    assert_contains(
        script,
        "shasum -a 256 -c -",
        "shell installer must run shasum in SHA-256 verification mode",
    );
    assert_contains(
        script,
        "requires 'sha256sum' or 'shasum' on PATH to verify SHA256SUMS",
        "shell installer must fail clearly when no SHA-256 verifier exists",
    );
    assert_contains(
        script,
        "need_command awk",
        "shell installer must require awk for strict two-field checksum parsing",
    );
}

#[test]
fn shell_installer_rejects_missing_duplicate_or_malformed_checksum_entries() {
    let script = include_str!("../scripts/install-latest.sh");

    assert_contains(
        script,
        "malformed SHA256SUMS entry for ${ASSET}",
        "shell installer must reject malformed selected checksum entries",
    );
    assert_contains(
        script,
        "SHA256SUMS does not contain checksum for ${ASSET}",
        "shell installer must reject missing selected checksum entries",
    );
    assert_contains(
        script,
        "SHA256SUMS contains duplicate checksum entries for ${ASSET}",
        "shell installer must reject duplicate selected checksum entries",
    );
    assert_contains(
        script,
        "is_sha256",
        "shell installer must validate that the selected checksum is a SHA-256 hex digest",
    );
    assert_contains(
        script,
        "${filename#\\*}",
        "shell installer must accept GNU sha256sum binary-mode *filename entries",
    );
    assert_contains(
        script,
        "awk 'NF == 2 { print $1; exit 0 } { exit 1 }'",
        "shell installer must parse checksum rows as exactly two whitespace-delimited fields",
    );
}

#[test]
fn shell_installer_verifies_before_extraction_install_or_execution() {
    let script = include_str!("../scripts/install-latest.sh");

    assert_contains(
        script,
        "cd \"$TMPDIR\"",
        "shell verifier must run from the temp directory so SHA256SUMS filenames resolve locally",
    );
    assert_contains(
        script,
        "checksum mismatch for ${ASSET}",
        "shell installer must fail clearly on archive checksum mismatch",
    );
    assert_sequence(
        script,
        &[
            "curl -fL \"https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS\"",
            "curl -fL \"https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}\"",
            "extract_expected_sha256 \"${TMPDIR}/SHA256SUMS\"",
            "verify_archive_checksum \"${TMPDIR}/${ASSET}\" \"${TMPDIR}/SHA256SUMS\"",
            "tar -xzf \"${TMPDIR}/${ASSET}\"",
            "install -m 0755",
            "\"${INSTALL_DIR}/codex-image\" --help >/dev/null",
        ],
        "shell installer must download checksums, verify, then extract/install/execute in order",
    );
}

#[test]
fn powershell_installer_downloads_sha256sums_and_requires_get_file_hash() {
    let script = include_str!("../scripts/install-latest.ps1");

    assert_contains(
        script,
        "/releases/download/$Version/SHA256SUMS",
        "PowerShell installer must download release checksum metadata",
    );
    assert_contains(
        script,
        "Get-Command Get-FileHash",
        "PowerShell installer must require Get-FileHash before verification",
    );
    assert_contains(
        script,
        "Get-FileHash -Algorithm SHA256",
        "PowerShell installer must compute SHA-256 over the downloaded archive",
    );
    assert_contains(
        script,
        "requires Get-FileHash to verify SHA256SUMS",
        "PowerShell installer must fail clearly when hash verification is unavailable",
    );
}

#[test]
fn powershell_installer_rejects_bad_checksum_state_and_mismatch() {
    let script = include_str!("../scripts/install-latest.ps1");

    assert_contains(
        script,
        "malformed SHA256SUMS entry for $Asset",
        "PowerShell installer must reject malformed selected checksum entries",
    );
    assert_contains(
        script,
        "SHA256SUMS does not contain checksum for $Asset",
        "PowerShell installer must reject missing selected checksum entries",
    );
    assert_contains(
        script,
        "SHA256SUMS contains duplicate checksum entries for $Asset",
        "PowerShell installer must reject duplicate selected checksum entries",
    );
    assert_contains(
        script,
        "checksum mismatch for $Asset",
        "PowerShell installer must reject archive checksum mismatches",
    );
    assert_contains(
        script,
        "ToLowerInvariant()",
        "PowerShell installer must compare hashes case-insensitively",
    );
    assert_contains(
        script,
        ".StartsWith('*')",
        "PowerShell installer must accept GNU sha256sum binary-mode *filename entries",
    );
    assert_contains(
        script,
        ".Substring(1)",
        "PowerShell installer must strip only the binary-mode marker from checksum filenames",
    );
}

#[test]
fn powershell_installer_verifies_before_extraction_install_or_execution() {
    let script = include_str!("../scripts/install-latest.ps1");

    assert_sequence(
        script,
        &[
            "Invoke-WebRequest \"https://github.com/$Repo/releases/download/$Version/SHA256SUMS\"",
            "Invoke-WebRequest \"https://github.com/$Repo/releases/download/$Version/$Asset\"",
            "Assert-ArchiveChecksum -ZipPath $ZipPath -ChecksumPath $ChecksumPath -Asset $Asset",
            "Expand-Archive -Path $ZipPath",
            "Copy-Item (Join-Path $TempDir \"$ArchiveRoot\\codex-image.exe\")",
            "& $BinaryPath --help | Out-Null",
        ],
        "PowerShell installer must download checksums, verify, then extract/install/execute in order",
    );
}
