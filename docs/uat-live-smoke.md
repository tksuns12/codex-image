# Live UAT smoke runbook (`generate` via installed Codex)

This runbook is for maintainers who need a **real Codex-backed** smoke test of `codex-image`.

Post-read action: you can run one guarded command that verifies local installation, Codex-backed image generation, copied image artifacts, and manifest output.

## Safety and billing warnings

- This flow is **opt-in** and asks Codex to generate a real image.
- It may consume whatever quota/limits apply to your logged-in Codex account.
- The script is single-shot: no retry loops are added by the wrapper.
- `codex-image` does not accept custom auth/API URL bases and does not call URL-configured OpenAI endpoints.

## Prerequisites

- Rust/Cargo installed.
- Codex installed and logged in.
- Codex can use its built-in image generation tool.
- Run from repository root.

If Codex is not on `PATH`, set `CODEX_IMAGE_CODEX_BIN` to the Codex executable. Otherwise `codex-image` also checks common VS Code/Cursor extension install locations.

## Run the guarded smoke check

```bash
CODEX_IMAGE_RUN_LIVE=1 bash scripts/uat-live-smoke.sh
```

Guard behavior:

- If `CODEX_IMAGE_RUN_LIVE=1` is not set, the script fails immediately before generate.
- This lets CI and default local runs avoid accidental live image calls.

## What the script verifies

The script prints phase logs and fails fast on any contract break:

1. Creates temporary install/output roots.
2. Installs local binary with `cargo install --path . --root <temp> --force`.
3. Runs:
   - `generate "UAT smoke image from codex-image" --out <temp-out> --output json`
4. Parses stdout JSON (explicitly emitted by `--output json`) and validates:
   - `manifest_path` exists and matches `<temp-out>/manifest.json`
   - `manifest.json` is valid JSON
   - every declared image path exists

## Expected successful observations

- Phase logs progress through guard, install, generate, and manifest validation.
- Manifest validation reports a non-zero image count.
- Final message indicates successful completion and cleanup.

## Failure interpretation

- `phase=guard`: opt-in variable missing; rerun with `CODEX_IMAGE_RUN_LIVE=1` only if live test is intended.
- `phase=codex-check`: configured Codex executable is missing or not executable.
- `phase=generate`: Codex-backed generation failed; script prints redacted CLI stderr envelope and exits non-zero.
- `phase=manifest-validate`: stdout or manifest contract malformed/missing.

## Cleanup

- Temporary install/output directories are removed automatically on success or failure.
- The script does not persist generated images after completion because output uses a temp directory.

## Manual fallback (if you need step-by-step debugging)

1. Install local binary to a temp root.
2. Ensure Codex itself works and is logged in.
3. Run `generate ... --out <dir> --output json`, then inspect stdout JSON, `manifest.json`, and image files.

Use normal redaction discipline: do not print tokens, raw auth files, bearer headers, raw upstream bodies, or raw base64 payloads.
