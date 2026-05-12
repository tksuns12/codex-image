# codex-image

[한국어](README.ko.md)

`codex-image` is a small CLI that asks an installed Codex CLI to generate an image with Codex's built-in image tool, then copies the result into a requested output directory and writes a manifest.

If you are new, read this page in order: verify Codex prerequisites, install `codex-image`, run one generation command, then confirm the output files/stdout.

It does **not** implement its own OpenAI OAuth flow, does **not** call URL-configured image API endpoints, and does **not** read or mutate Codex auth files. Codex itself owns login and image generation access.

## Prerequisite: Codex CLI / Codex extensions

`codex-image generate` depends on a working Codex installation that can already generate images.

- The standalone Codex CLI is currently **macOS-only**.
- Codex installs provided by **VS Code**/**Cursor** extensions are also supported for `codex-image generate`.

Executable resolution order:

1. `CODEX_IMAGE_CODEX_BIN` when set.
2. `codex` on `PATH`.
3. Common VS Code/Cursor Codex extension install locations.

Codex must already be logged in and able to use its built-in image generation tool.

## Install

Recommended path: install from a release artifact for your platform.

### From a release artifact

Download and run the installer script for your platform. Each script resolves the newest GitHub Release tag automatically, verifies the downloaded archive against `SHA256SUMS`, installs the binary, and verifies `codex-image --help`.

#### Linux x86_64 / macOS x86_64 / macOS arm64

```bash
curl -fsSL https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.sh | sh
```

The script installs to `${HOME}/.local/bin` by default. Override with `CODEX_IMAGE_INSTALL_DIR=/path/to/bin` and make sure the install directory is on your `PATH`.

#### Windows x86_64 PowerShell

```powershell
Invoke-RestMethod https://raw.githubusercontent.com/tksuns12/codex-image/release/scripts/install-latest.ps1 | Invoke-Expression
```

The script installs to `$HOME\bin` by default. Override with `$env:CODEX_IMAGE_INSTALL_DIR = "C:\path\to\bin"` before running it and make sure the install directory is on your `PATH`.

### From source (secondary path)

Use this only when you intentionally want to install from the current checkout (for local development/testing).

```bash
cargo install --path . --force
codex-image --help
```

## Generate images + manifest

Run one generation with an output directory:

```bash
codex-image generate "A watercolor fox reading in a library" --out ./out
```

Expected output from that single command:
- an image file named `image-0001.<format>` in `./out`
- `manifest.json` in `./out`
- a human-readable success line on stdout (default behavior)

`manifest.json` is always written under `--out`. If your automation needs machine-readable manifest stdout, rerun with `--output json`:

```bash
codex-image generate "A watercolor fox reading in a library" --out ./out --output json
```

Use `--quiet` when your caller checks files and exit status separately and does not need success stdout text.

## After your first run

If your first command produced `image-0001.<format>` and `manifest.json`, you're done with the quickstart.
The sections below are optional follow-up material for agent automation, skill maintenance, and binary updates.

If you're curious about execution details: `codex-image` runs `codex exec`, asks Codex to use its built-in image tool, reads Codex's final JSON response, and copies the generated image into your output directory.

## Post-first-run references (optional)

If your first run succeeded, use these references for operations beyond quickstart:

- Advanced operations guide (skill lifecycle, automation prompt, update behavior, verification posture): [docs/advanced-reference.md](docs/advanced-reference.md)
- Canonical supported tool/path/source matrix: [docs/skill-paths.md](docs/skill-paths.md)
- Intentional live Codex-backed smoke runbook: [docs/uat-live-smoke.md](docs/uat-live-smoke.md)

Fast command reference:

```bash
codex-image generate "A watercolor fox reading in a library" --out ./out --quiet
codex-image generate --prompt-file ./prompt.txt --out ./out --timeout 120 --debug-diagnostics
codex-image skill install --tool codex --scope project --yes
codex-image skill update --tool codex --scope project --yes
codex-image update --dry-run --output json
codex-image update
codex-image update --version v1.2.3
```

Keep using explicit `--tool` and `--scope` values for skill automation. Use `--output json` when a caller needs machine-readable stdout, and use `codex-image update --dry-run --output json` before replacement when you want a non-mutating JSON preview.
