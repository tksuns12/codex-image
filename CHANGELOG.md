# Changelog

## [0.4.0](https://github.com/tksuns12/codex-image/compare/v0.3.0...v0.4.0) (2026-05-27)


### Features

* Added `generate --timeout <SECS>` with positive-value clap valida… ([c227164](https://github.com/tksuns12/codex-image/commit/c2271642a285ba4f184e13e217e8dd7536ae8cdc))
* Added prompt-file batch generation to `codex-image generate` with… ([f139b9e](https://github.com/tksuns12/codex-image/commit/f139b9ef205e47b549bcd9e49adbc976c0f807e8))
* Enforce updater SHA256SUMS verification ([cd5322e](https://github.com/tksuns12/codex-image/commit/cd5322e75438db3f16e548394f138dc2e79a3780))
* Made `update` success output mode-aware (human/json/quiet) at the… ([9d2102f](https://github.com/tksuns12/codex-image/commit/9d2102f5d779ba3591665bd92b34911a11c48007))
* Refreshed managed codex-image skill guidance for M005 automation… ([20f1af3](https://github.com/tksuns12/codex-image/commit/20f1af3cc2bbf933cc5c608969cd6028465a5aa4))
* Validated and finalized skill install success rendering so it emi… ([6460172](https://github.com/tksuns12/codex-image/commit/6460172fad6cc408239bf676a2b58a523e530152))
* Wire single-generation diagnostics through Codex subprocess telem… ([a4f4847](https://github.com/tksuns12/codex-image/commit/a4f48475d76752ea91c704a6b2d9b8aef5b13a36))


### Bug Fixes

* Ran the full S03 regression gate, fixed a formatting regression in… ([88d9a45](https://github.com/tksuns12/codex-image/commit/88d9a4596605ddcaa9ddb3bd40e6424df39eeafb))
* Ran the full timeout-focused + full-suite proof, fixed the only re… ([47c6cf1](https://github.com/tksuns12/codex-image/commit/47c6cf18fcc0e0c2115a1e2668ce11e1bf528841))

## [0.3.0](https://github.com/tksuns12/codex-image/compare/v0.2.1...v0.3.0) (2026-05-07)


### Features

* Expanded the managed codex-image skill template into a compact Op… ([62307a5](https://github.com/tksuns12/codex-image/commit/62307a5dd397ec3a73d15e806fb50471b7511106))
* Moved managed codex-image skill prose into a checked-in compile-t… ([3a1cde6](https://github.com/tksuns12/codex-image/commit/3a1cde68772c2af6169eb2f10a2693c4d6ec4dc0))


### Bug Fixes

* **release:** use canonical repository URLs ([75c7970](https://github.com/tksuns12/codex-image/commit/75c797024718855b38deb85a9f780fdbfe5d9c54))

## [0.2.1](https://github.com/tksuns12/codex-image/compare/v0.2.0...v0.2.1) (2026-05-07)


### Bug Fixes

* **update:** apply latest release by default ([93b83e4](https://github.com/tksuns12/codex-image/commit/93b83e4c8d37c63ccba2029240a4f7dc48768449))

## [0.2.0](https://github.com/tksuns12/codex_image/compare/v0.1.0...v0.2.0) (2026-05-06)


### Features

* Added a managed `skill update` CLI lifecycle with shared target-s… ([d08484b](https://github.com/tksuns12/codex_image/commit/d08484b81eec69c059a7e112fdaf4bd23d710afa))
* Added an interactive Space/Enter skill-install target selector (d… ([311dd9d](https://github.com/tksuns12/codex_image/commit/311dd9d82c4c1c5ddb55fbbea396ed5b707cb3e5))
* Added deterministic managed SKILL.md generation and conservative… ([a73cfa6](https://github.com/tksuns12/codex_image/commit/a73cfa69847751cfbbc497bbe514bd3c94f0d6c4))
* Added fixture-testable updater orchestration that resolves/downlo… ([a19443f](https://github.com/tksuns12/codex_image/commit/a19443fabc78f37fc8c43d036c82c11a40fd6bcf))
* Added non-interactive `skill install` CLI wiring with typed tool/… ([a1b71d7](https://github.com/tksuns12/codex_image/commit/a1b71d7c2dea9d76ceee12eef5aea3b8d77012c0))
* **skill-install:** show install state and preselect installed targets ([59e5a6c](https://github.com/tksuns12/codex_image/commit/59e5a6c06a1f1b9ec69d3e12ceab01b37daa5bb2))
* **skill-install:** uninstall unchecked interactive targets safely ([c8e6ca4](https://github.com/tksuns12/codex_image/commit/c8e6ca46c2d0ce047804a6098265bae0cfbfaa3c))
* Switched Windows release ZIP packaging to preserve the top-level… ([3d2825b](https://github.com/tksuns12/codex_image/commit/3d2825ba6179b9abc2df26c47b2cb97c0b082e79))
* Wired `skill install` through repeatable `--tool/--scope` target… ([85119f1](https://github.com/tksuns12/codex_image/commit/85119f1102519f9b31ead3d46e29bd202bc1578d))


### Bug Fixes

* Ran the full S07 updater remediation verification stack, fixed a r… ([8a843bd](https://github.com/tksuns12/codex_image/commit/8a843bd7597c20dc6e38422c07e86c43791b82b7))
* Wired `codex-image update` into the top-level CLI with redacted di… ([0e0a35e](https://github.com/tksuns12/codex_image/commit/0e0a35e6a8ad9fa5c414f4f989d4dcf0f45254d9))

## 0.1.0 (2026-04-30)


### Features

* Added a fail-closed output writer that decodes generated images i… ([98cd004](https://github.com/tksuns12/codex_image/commit/98cd0045404cc10865d0a8838feb7722359aa3ff))
* Added a repeatable local install verifier script that installs th… ([512f4b0](https://github.com/tksuns12/codex_image/commit/512f4b00f4aada655d60cfc112647e8346e40995))
* Added an opt-in live UAT smoke script and runbook that validate l… ([21e30fc](https://github.com/tksuns12/codex_image/commit/21e30fcd470550a456b39c695b95b33a8390801c))
* Added auth lifecycle primitives for safe status inspection, idemp… ([ed92b23](https://github.com/tksuns12/codex_image/commit/ed92b2348509502e113832dbdcda2471b5bbb69b))
* Hardened diagnostics contracts with exact JSON-envelope shape che… ([7266984](https://github.com/tksuns12/codex_image/commit/7266984cf85efae51b0606790a4d9a437e90dbdb))


### Bug Fixes

* **auth:** request image generation scope ([2503731](https://github.com/tksuns12/codex_image/commit/25037318284176e6be4f0b31d663225cfb7b88d1))
* **auth:** use OAuth callback login ([57779c6](https://github.com/tksuns12/codex_image/commit/57779c68bd3bf8ce50fec3991d0ed9968403b106))
* **auth:** use OpenAI auth host for login ([5c5a099](https://github.com/tksuns12/codex_image/commit/5c5a099034ee0483e8b8e26fc6a20c64a628f2b6))
* **generate:** remove direct OpenAI backend ([1bef7e8](https://github.com/tksuns12/codex_image/commit/1bef7e860f331775faa8815d9c4d192156331147))
* **generate:** use Codex image backend by default ([6b9931f](https://github.com/tksuns12/codex_image/commit/6b9931f1d61fb28f71d73ee6b57c001530f06e2d))
* harden auth status redaction ([9f60efd](https://github.com/tksuns12/codex_image/commit/9f60efda7d7c1bccdb9e5ac7900ab3080b495f1e))

## Changelog

All notable changes to this project will be documented in this file.

This project uses release-please to update this changelog from Conventional Commit history when release PRs are prepared from the `release` branch.
