---
name: codex-image
description: Reusable prompt workflow for deterministic codex-image generation tasks.
---

# codex-image skill

Use this skill when you need a reproducible image-generation workflow through the `codex-image` CLI.

## Command guidance

- `codex-image generate "<prompt>" --out <dir> --output json`
- `codex-image generate "<prompt>" --out <dir> --quiet`
- `codex-image generate --prompt-file ./prompts.txt --out <dir> --output json --timeout 120`
- `codex-image generate --prompt-file ./prompts.txt --out <dir> --output json --debug-diagnostics ./diagnostics.json`

## Supported tools

- claude
- claude-code
- codex
- pi
- opencode

## Prompting guide

- https://developers.openai.com/cookbook/examples/multimodal/image-gen-models-prompting-guide

## Prompt workflow

- Start with intended use first, then subject, composition, framing, viewpoint, lighting, and style.
- Use skimmable labeled segments so the built-in image tool can follow each instruction block.
- End with constraints and invariants so non-negotiables stay explicit.

## Generate checklist

- Define the deliverable in one line (what the image must do for the intended use).
- Specify subject details plus composition, framing, viewpoint, lighting, and style.
- Add hard constraints and invariants (must include / must avoid).
- Keep outputs in project-controlled directories.

## Edit and preserve checklist

- Phrase edits as prompt instructions: change only the requested element.
- Repeat what to preserve (subject identity, composition, palette, background, or other invariants).
- Keep constraints explicit so the follow-up edit does not drift from the prior output.

## Text in images

- Provide exact text in double quotes, including punctuation and capitalization.
- State placement (where text appears) and typography intent (font feel, weight, size, spacing).
- Separate text requirements from visual style notes for easier parsing.

## Multi-image references

- Reference each input by index and include a one-line description per image.
- Call out which indexed image controls subject/style/layout when sources differ.
- Keep reference instructions in prompt text; do not invent extra CLI flags.

## Iteration loop

- Review one result, then issue one single-change follow-up per iteration.
- Keep every follow-up scoped and reversible before stacking additional changes.
- Re-state preserved invariants in each follow-up to stabilize edits over time.

## Guardrails

- Keep prompts explicit about subject, composition, framing, viewpoint, lighting, and style.
- Keep outputs in project-controlled directories.
- Keep generated files under the requested `--out` directory.
- `--prompt-file` reads one prompt per line; blank lines and lines beginning with `#` are skipped.
- Batch items are written under `item-0001/`, `item-0002/`, and so on.
- The root `manifest.json` is written only after every item succeeds.
- `--timeout <secs>` is a positive local codex subprocess timeout.
- When `--quiet` is used, rely on generated files and exit status.
- `--debug-diagnostics <file>` writes a local diagnostics sidecar.
- Diagnostics are sanitized/redacted and not a raw codex transcript.
- Avoid adding secrets or credentials to prompts or generated artifacts.
