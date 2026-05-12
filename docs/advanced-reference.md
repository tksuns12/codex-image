# Advanced reference: agent workflows, generation contracts, updates, and verification

## Purpose and reader

This document is for post-first-run maintainers and agent operators who have already completed the quickstart and now need deterministic operations. After reading, you should be able to install or update skills, automate generation safely, inspect manifests and diagnostics, validate binary updates, and choose no-live versus live verification intentionally.

Canonical supported-tool/path/source matrix: [docs/skill-paths.md](docs/skill-paths.md)

## Success output and failure envelope

Use this section when wiring `codex-image` into scripts or agent workflows.

- The default success output is human-readable.
- `--output json` emits one aggregate JSON object on success.
- `--quiet` suppresses success stdout; errors still go to stderr.
- Non-clap failures use a centralized redacted error envelope on stderr with an error code, message, recoverability flag, and hint.

Generation examples:

```bash
codex-image generate "A watercolor fox reading in a library" --out ./out
codex-image generate "A watercolor fox reading in a library" --out ./out --output json
codex-image generate "A watercolor fox reading in a library" --out ./out --quiet
```

Skill install, skill update, and binary update commands follow the same output posture. For skill automation, JSON output is one object with `operation` and `results`; each result uses stable high-level fields: `tool`, `scope`, `status`, `target_path`.

## Image generation and batch contracts

Use this section when automating single or prompt-file generation.

Single prompt generation:

```bash
codex-image generate "A clean product render of a brass desk lamp" --out ./out --output json
```

Prompt-file batch generation:

```bash
codex-image generate --prompt-file ./prompts.txt --out ./batch-out --output json --timeout 120
```

Prompt-file rules:

- `--prompt-file` reads one prompt per line.
- Lines are trimmed before use.
- blank lines and lines beginning with `#` are skipped.
- Items run sequentially; no concurrency or parallel throughput is promised.
- Each item writes under `item-0001/`, `item-0002/`, and so on.
- Each successful item has its own `manifest.json`.
- The root `manifest.json` is written only after every item succeeds.
- A stale root `manifest.json` is removed before batch work starts.
- If a later item fails, completed item directories remain for inspection.

The success JSON for batch mode is the same aggregate object written to the root manifest. It includes the prompt file, item count, per-item output directories, per-item manifest paths, images, and sanitized response metadata.

## Timeout and source-path trust boundary

`--timeout <secs>` is a positive local Codex subprocess timeout. The value controls how long `codex-image` waits for the local Codex subprocess; it is not forwarded to Codex, and neither `--timeout` nor its numeric value should appear in the Codex argv.

Image source paths reported by Codex are trusted only as current-run artifacts. If Codex reports a missing path, invalid path, metadata-unreadable path, or a path that predates current invocation, `codex-image` fails closed with `response_contract.image_generation`. On that failure, it does not copy a stale image and does not write a false success manifest. This is a freshness and response-contract boundary, not cryptographic provenance.

## Debug diagnostics sidecar

Use `--debug-diagnostics <FILE>` when an agent needs a durable local inspection surface for generation failures or batch progress.

```bash
codex-image generate --prompt-file ./prompts.txt --out ./batch-out --output json --debug-diagnostics ./diagnostics.json
```

Diagnostics contract:

- `--debug-diagnostics <FILE>` writes JSON with schema `codex-image.generation-diagnostics` and schema version 1.
- The sidecar includes safe modes, counts, statuses, and placeholders: invocation mode, result, prompt source, stdout mode, timeout seconds, batch planned/completed/failed indexes, per-run status, timeout state, exit code, final-message parse status, and classified failure information.
- The sidecar includes no raw prompts, prompt hashes, raw stdout/stderr, token sentinels, final-message payloads, credentials, sensitive paths, image payloads, or exact Codex environment sentinel.
- If the requested diagnostics sidecar cannot be written, the command fails closed with `filesystem.output_write_failed`.
- Diagnostics do not change stdout semantics: human, JSON, and quiet success modes still behave as requested.

## Skill install and update workflows

Use this section when maintaining `SKILL.md` installs across supported tools and scopes.

### Interactive install (`Space` / `Enter`)

```bash
codex-image skill install
```

Interactive behavior:

- Use `Space` to toggle selections.
- Use `Enter` to confirm selections.
- Already-installed targets are preselected.
- Outdated managed installs are preselected and labeled `installed:outdated`.
- Manual/tampered installs are preselected and labeled `installed:protected`.
- Unchecking an installed managed target removes that `SKILL.md`.
- Unchecking a manual/tampered target is blocked by default; pass `--force` to allow removal.

### Deterministic install commands (agent/CI)

```bash
codex-image skill install --tool codex --tool pi --scope project --yes
codex-image skill install --tool claude-code --scope global --yes
codex-image skill install --tool opencode --scope project --yes
```

Use explicit `--tool` slugs, explicit `--scope`, and `--yes` for non-interactive automation.

### Skill updates

Interactive/default:

```bash
codex-image skill update
```

Deterministic scoped update:

```bash
codex-image skill update --tool codex --scope project --yes
```

Managed update behavior:

- Creates missing managed files.
- No-ops up-to-date managed files.
- Refreshes outdated managed files to current bundled content.
- Blocks manual/tampered files by default.
- Requires `--force` as the explicit overwrite escape hatch for blocked/tampered targets.
- Human output is meant for operators; `--output json` is the automation contract.

## Agent auto-install prompt

Use this prompt when delegating setup to an autonomous agent:

```text
Inspect the current project and choose supported tools/scopes for codex-image skills.
Run only non-interactive commands with explicit confirmation:
- codex-image skill install --tool <slug> --scope <project|global> --yes
- codex-image skill update --tool <slug> --scope <project|global> --yes
Do not mutate authentication state, do not run credential flows, and do not change credentials.
Optionally run codex-image update --dry-run before any binary replacement.
```

## Binary update behavior

`codex-image update` uses GitHub Release artifacts and supports latest-by-default apply, dry-run validation, and explicit version pinning.

```bash
codex-image update --dry-run
codex-image update
codex-image update --version v1.2.3
```

Checksum contract:

- Each release publishes one aggregate `SHA256SUMS` asset alongside platform archives.
- Updater and install scripts require exactly one selected archive entry for the current platform archive.
- Verification happens before archive validation, dry-run success, extraction, replacement, install, or execution.
- malformed, missing, duplicate, or mismatched checksum metadata fails closed.
- The checksum file and selected archive come from the same GitHub Release channel.
- signatures and provenance attestations are out of scope.

Windows same-process replacement limitation: on Windows, do not assume in-process overwrite; prefer `codex-image update --dry-run` followed by manual replacement guidance if the current executable cannot be replaced in place.

## Verification posture

### No-live verification (default for routine maintenance)

Use this posture when you want contract confidence without external side effects:

- no live GitHub downloads
- no live Codex generation
- no credentials
- no auth mutation

Local install contract check (no live generation):

```bash
bash scripts/verify-local-install.sh
```

S06 final closeout captures the integrated no-live proof in `target/s06-closeout.log`.

### Live smoke verification (intentional Codex-backed run)

Use this only when you explicitly want a real Codex image generation smoke test:

- Runbook: [docs/uat-live-smoke.md](docs/uat-live-smoke.md)
- Guarded command:

```bash
CODEX_IMAGE_RUN_LIVE=1 bash scripts/uat-live-smoke.sh
```

## Prompting guidance for installed skill content

When authoring or updating `SKILL.md` content, follow the OpenAI multimodal image prompting guide:

- https://developers.openai.com/cookbook/examples/multimodal/image-gen-models-prompting-guide

## Related references

- Quickstart and first-run flow: `README.md`
- Canonical tool/path/source matrix: [docs/skill-paths.md](docs/skill-paths.md)
- Live Codex-backed smoke runbook: [docs/uat-live-smoke.md](docs/uat-live-smoke.md)
