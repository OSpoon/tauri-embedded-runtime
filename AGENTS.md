# Project Contribution and Agent Guidelines

## Project Overview

This repository contains a Tauri 2 desktop application built with Vue 3, TypeScript, and
Rust. The application prepares and manages private Python and Node.js runtimes together
with local business services. Runtime files, service dependencies, caches, and logs must
remain inside the application's private data directory and must not modify the host
system's `PATH`, shell configuration, or global package directories.

## Technology and Version Requirements

- Use pnpm as the only JavaScript package manager. The required version is declared by the
  `packageManager` field in `package.json`.
- Use the Node.js version declared in `.nvmrc`.
- Use the Rust toolchain declared in `rust-toolchain.toml`.
- Keep GitHub Actions on Node.js 24-compatible releases: `actions/checkout@v7`,
  `actions/setup-node@v7`, and `pnpm/action-setup@v6.1.0`.
- Keep the versions in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`
  synchronized. These files are updated together by `bumpp`. Let Cargo maintain
  `src-tauri/Cargo.lock` automatically; do not hand-edit its package versions.
- Keep prerelease versions compatible with the Windows packaging policy: stable tags build all
  configured bundles, while prerelease Windows builds use NSIS because WiX/MSI requires a
  numeric installer version.
- Keep the updater signing public key in `src-tauri/tauri.conf.json`. Never commit the private
  key under `.tauri/`; provide it to release builds through `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` GitHub Actions secrets.
- Keep dependency changes in both `package.json` and `pnpm-lock.yaml`. Do not hand-edit the
  lockfile when the package manager can regenerate it.

## Repository Boundaries

- `src/`: Vue 3 frontend and TypeScript application code.
- `src-tauri/`: Tauri configuration, Rust backend, runtime manager, tests, and bundled
  project resources.
- `src-tauri/resources/projects/<project-id>/`: self-contained Python or Node.js demo
  services. Each project must have its own `project.json` and dependency definition.
- A project's private cache must contain only the package-manager cache required by its
  service: `cache/pip` for Python projects or `cache/npm` for Node.js projects.
- `src-tauri/resources/runtime-artifacts.json`: pinned runtime artifacts. Any change must
  be reviewed for platform, architecture, version, URL, archive format, and SHA-256
  consistency.
- `docs/`: development and integration documentation.
- `.github/workflows/`: CI and release automation.

Do not directly edit generated or disposable directories:

- `src-tauri/gen/`
- `src-tauri/target/`
- `dist/`
- `.mypy_cache/`
- `node_modules/`

Preserve unrelated user changes. Do not use destructive Git commands such as
`git reset --hard` or `git checkout --` unless explicitly requested.

## Implementation Requirements

- Prefer small, focused changes that preserve existing runtime lifecycle behavior,
  cancellation behavior, retry behavior, and service cleanup behavior.
- Keep frontend imports, formatting, and stylistic rules compatible with
  `@antfu/eslint-config`: double quotes, no semicolons, and multiline trailing commas.
- Use Vue Composition API and TypeScript for frontend changes. Keep public component props,
  emitted events, and composable return types explicit when practical.
- Use Rust 2021 edition and format Rust with `rustfmt --edition 2021`.
- Do not weaken TypeScript strictness, ESLint rules, Rust warnings, or test coverage merely
  to make a change pass.
- Do not introduce host-global installation logic or network behavior that bypasses the
  runtime artifact manifest and checksum verification.

## Required Verification

Run the checks relevant to the change. Before merging a normal code change, run at least:

```bash
pnpm lint
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
```

The pre-commit hook is installed by `simple-git-hooks` and runs `lint-staged`. It formats
staged JavaScript, TypeScript, and Vue files with ESLint and staged Rust files with
`rustfmt`. If a hook changes a file, review the result and stage the updated file again.

## Git Commit Message Specification

All normal commits must use the Conventional Commits format:

```text
<type>(<scope>): <subject>
```

Allowed types are:

- `feat`: a user-visible feature
- `fix`: a user-visible bug fix
- `perf`: a performance improvement
- `refactor`: an internal restructuring without intended behavior change
- `docs`: documentation-only change
- `style`: formatting or non-functional style change
- `test`: test changes
- `build`: build or dependency packaging changes
- `ci`: continuous integration or release workflow changes
- `chore`: maintenance work
- `revert`: reverting an earlier commit

Commit message rules:

- Use a lowercase type and an optional lowercase or kebab-case scope, such as `runtime`,
  `ui`, `release`, or `ci`.
- Write the subject in the imperative mood, keep it concise, and do not end it with a
  period. Keep it at or below 72 characters when possible.
- Add a body after one blank line when context, motivation, compatibility impact, or
  migration details are needed. Do not repeat the diff.
- Mark breaking changes with `!` after the type or scope and include a footer such as
  `BREAKING CHANGE: explain the migration required`.
- Put issue or pull request references in the footer, for example `Fixes #123` or
  `Refs: #123`.
- Keep unrelated changes in separate commits.

Examples:

```text
feat(runtime): support repairing a single runtime component
fix(ui): preserve the previous setup result after retry
docs(release): document draft release verification
ci: build Tauri bundles for supported platforms
```

The release commit is an automated exception and must use `release: v<version>`. It is
created by `bumpp`; do not create or rewrite it manually.

Commit messages must follow this format so the project history can be grouped reliably by the
changelog generator. Commits with an unsupported or malformed prefix may be omitted from release
summaries.

## CHANGELOG and Release Automation

`bumpp` is the single local release entry point and is responsible for:

1. Selecting or receiving the new semantic version.
2. Updating `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
3. Running Cargo through bumpp's `execute` command so Cargo can refresh `src-tauri/Cargo.lock`
   automatically.
4. Running the standard npm `version` lifecycle hook after the version files are updated. That
   hook invokes `changelogen`, whose configuration reads the updated package version and generates
   the corresponding entry in the root `CHANGELOG.md`.
5. Creating the `release: v<version>` commit, including the generated changelog and Cargo's
   generated lockfile update.
6. Creating the `v<version>` tag and pushing the commit and tag.

`changelogen` is the only source of release entries in `CHANGELOG.md`. Never hand-edit release
entries or add policy text to that file. The entries are generated from Conventional Commits;
commits with unsupported or malformed prefixes may be omitted. The generator runs on the local
machine as part of `pnpm release`; it does not need a GitHub Actions job.
Keep bumpp lifecycle scripts enabled during releases; `--ignore-scripts` skips the `version` hook
and therefore skips changelog generation.

The tag-triggered workflow builds and uploads the enabled platform artifacts and creates a Draft
Release. With `bundle.createUpdaterArtifacts: true`, it also signs the updater artifacts and
publishes the generated `latest.json` metadata. It does not generate or maintain the changelog.
Use the generated `CHANGELOG.md` entry when reviewing the Draft Release description before
publishing it.

Use the following workflow:

1. Ensure the branch is up to date, the working tree is clean, and all required checks pass.
2. Run `pnpm release` and select `patch`, `minor`, `major`, or an explicit version.
3. Review all synchronized version files, the generated `CHANGELOG.md` entry, and the release
   summary before confirming.
4. The `v<version>` tag triggers `.github/workflows/release.yml`.
5. The workflow builds all enabled matrix platform artifacts and creates a Draft Release. Intel
   macOS and Linux entries are kept commented in the matrix by default and must be explicitly
   enabled when needed.
6. Review the platform artifacts, generated release entry, and signature status in the GitHub Draft
   Release before publishing it.

The application updater is native-only. Do not add a frontend updater page or JavaScript updater
composable unless the product requirements explicitly change. The native “检查更新…” menu item
handles checking, user confirmation, signed download, installation, and restart through the Rust
updater and dialog plugins. The endpoint is the current repository's stable GitHub Release
`latest.json`; prerelease releases are not treated as the stable update channel.

Do not create release tags by hand. That bypasses the Tauri version synchronization and the
repository's Draft Release workflow.

## Change Safety and External Effects

- Do not run `git commit`, `git push`, publish a GitHub Release, or change remote settings
  unless the user explicitly requests that operation. `pnpm release` is intentionally an
  external write because it commits, tags, and pushes.
- Before a release, verify that no unrelated changes are present. The `bumpp` configuration
  uses `all: true` so its release commit also contains the Cargo-generated `Cargo.lock` update.
- Treat runtime downloads, process execution, filesystem writes, and service lifecycle
  operations as security-sensitive. Preserve local-only binding, checksum verification,
  cancellation, cleanup, and rollback behavior.
