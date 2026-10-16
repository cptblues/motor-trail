# Native Distribution Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or
> superpowers:executing-plans task by task. Preserve unrelated work; do not tag
> or publish a release during implementation.

**Goal:** Produce three native archives and publish them only for pushed v* tags.

**Architecture:** A central runtime path resolver separates immutable installation
resources from mutable OS user data. Standard-library packaging scripts and a
separate release workflow validate all three outputs before publication.

**Tech Stack:** Rust/Bevy, Python standard library, GitHub Actions, GitHub CLI.

## Task 1: Runtime Paths And Migration

Files: `src/app_paths.rs`, `src/app_paths/`, `src/main.rs`,
`src/terrain/persistence.rs`, `src/terrain/runtime.rs`.

- [x] Add failing tests for executable-relative packages, macOS bundles, absent
  resources, development fallback and Windows/macOS/Linux user-data locations.
- [x] Implement pure `ResourceLayout::resolve(exe, development, distribution)`
  and environment collection outside tests; no process-global cwd/env mutation.
- [x] Test then implement non-overwriting, bounded initial copying: legacy files
  first, missing defaults second, last-profile marker retained; retry errors and
  mark completion only after success. Never copy the mounts JSON as a profile.
- [x] Wire absolute Bevy assets, writable ProfileStore/TerrainStore roots and
  startup diagnostics; retain existing explicitly-rooted store tests.
- [x] Add `--check-installation` for JSON/GLB validation without GPU/window/data
  writes; `--user-data-dir <path>` isolates manual tests.

Commands: `cargo test --release app_paths`, then `cargo test --release`.
Expected: a witnessed failing regression before implementation, then all pass.

## Task 2: Packaging And Release Workflow

Files: `scripts/` packaging/validation/tests, `.github/workflows/release.yml`.

- [x] Write failing archive tests with fixture binaries for all target layouts,
  tag metadata, executable permissions, missing/corrupt resources and validation.
- [x] Implement standard-library packaging; exact requested archive names and
  package paths; no symlinks or user selection markers in shipped defaults.
- [x] Validate target architecture, resources and metadata before upload and
  again before release creation. Retain macOS executable mode in archives.
- [x] Add only push.tags.v* trigger, native architecture assertions, stable
  toolchain, target-specific cache and `cargo build --release --locked --target`.
- [x] Configure packaged `--check-installation` from a temporary working directory on
  each native runner. No graphics in CI. Windows static CRT avoids VC runtime
  installation; no proprietary certificates required.
- [x] Configure publication only after all matrix builds succeed, contents:write only in this
  job, using GITHUB_TOKEN; verify tag exists, draft/upload/validate before public
  release, alpha/beta/rc marked prerelease. No workflow_dispatch or PR triggers.

Commands: `python -m unittest discover -s scripts/tests -v`; workflow parser/lint.
Expected: valid Windows and both macOS fixtures, malformed archives rejected.

## Task 3: Integration, Docs And Acceptance

Files: `README.md`, `docs/distribution.md`, `.gitignore`, this plan.

- [x] Document downloads, unsigned-app warnings with no security bypass,
  data paths/migration, native manual tests and exact first-tag commands.
- [x] Preserve ordinary CI; if adding script tests there, no distribution build
  or packaging of real apps on main/PR. No remote execution during this mission.
- [x] Build and validate a real Windows package locally from an unrelated cwd.
- [x] Run fmt, Rust tests debug/release, release build --locked, script tests and
  git diff --check; independent review of data preservation and release gates.
- [x] Report exact verified results and unexecuted macOS/GitHub acceptance steps.

Work remains on the existing clean `codex/terrain-workshop` checkout, reusing its
shared Cargo cache. No new worktree or publication is needed for this request.

## Verification Record: 2026-10-16

- Rust debug and release: 316 tests passed in each mode.
- Python: 30 tests passed, including optional YAML parsing enabled by an isolated
  PyYAML installation outside the repository; no packaging runtime dependency.
- Regression RED/GREEN witnessed for malformed GLB data, unsupported sparse/matrix
  forms and conservative expanded-accessor budget, in addition to path/migration
  and archive contract regressions during implementation.
- Windows native static-CRT release build, real ZIP validation and extracted
  headless smoke passed. PATH contained only Windows system directories and an
  invalid BEVY_ASSET_ROOT could not override packaged resources.
- Final extracted graphical smoke exited 0: six captures, sculpt/JSON roundtrip,
  preview/rotation/confirmation/selection, physics panel and driving verified.
- Development cargo run resource check and executable check from an unrelated
  working directory both passed. Four pre-existing vehicle dead_code warnings
  remain; no physics code, dependencies, source resources or ordinary CI changed.
- Independent review findings addressed and final restricted review clear.
- macOS compilation, Finder/Gatekeeper and real GitHub execution are intentionally
  unexecuted: no tag, push or release was authorized for this implementation.
- Full result detail and modified-file inventory: `docs/distribution.md`.
