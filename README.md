<p align="center"><strong>Codexium CLI</strong> — a privacy-focused fork of OpenAI's Codex CLI that removes telemetry and adds a local, per-provider model customization layer.</p>
<p align="center">
  <img src="https://github.com/openai/codex/blob/main/.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>

Codexium is a fork of [OpenAI Codex](https://github.com/openai/codex). It keeps the Codex coding agent experience — a local CLI + TUI coding agent — but strips out telemetry, analytics, and crash/feedback reporting, and replaces the hard-coded model/auth handling with a configurable local overlay.

---

## What this fork removes

The following telemetry/analytics/observability machinery was removed from the codebase:

- **OpenTelemetry (otel) crate** — the entire `codex-rs/otel` crate and all `otel_*` modules, OTLP export, and metric emission.
- **Analytics crate** — the entire `codex-rs/analytics` crate (event tracking, facts, reducers).
- **Feedback crate** — the entire `codex-rs/feedback` crate and the app-server feedback-upload request processor, plus the TUI feedback view.
- **Per-crate metrics/telemetry modules** — telemetry files and metric emission removed across `codex-api`, `codex-client`, `core`, `core-plugins`, `exec-server`, `cloud-config`, `ext/goal`, `ext/memories`, `login`, `memories/write`, `mcp-tool-call`, and `guardian`.
- **Plugin metrics sidecar** — the plugin metrics sidecar process.

Net effect: Codexium does **not** send usage/analytics/telemetry data, and does **not** surface the feedback-upload flow. The `codex` binary is still fully functional as a local coding agent. (The `v8`/rusty_v8 crates remain in the workspace for the code-mode host, but the main `codex` CLI does not depend on them.)

> Note: some schema files still contain an `AnalyticsConfig` type or field names for wire-compatibility (e.g. `app-server-protocol/schema/...`) and `opentelemetry` strings may appear in build/Bazel metadata. These are inert type/field definitions, not active telemetry collection.

## What this fork adds

### Local model & auth overlay (`codexium`)

A new customization layer in `codex-rs/core/src/codexium.rs` reads per-provider metadata and API keys from a `codexium` folder under your Codex home directory:

- `<codex_home>/codexium/models.json` — per-provider, per-model overrides: `label`, `context_window`, `max_output_tokens`, and visibility.
- `<codex_home>/codexium/auth.json` — API keys keyed by provider id; values are injected into the process environment as the provider's `env_key` so existing auth resolution picks them up.

Both files are created automatically with sensible defaults if missing. See `codex-rs/core/src/providers_registry.json` for the built-in provider registry.

### CI / release automation

- `.github/workflows/build-and-release.yml` — a **Windows-only** build & release workflow:
  - Triggers on pushes to `test-**` branches, pushes to `v*` tags, and manual `workflow_dispatch`.
  - Builds two executables from the `codex-rs` workspace:
    - **dev** — `cargo build` (debug) → `codex-dev.exe`
    - **prod** — `cargo build --release` → `codex.exe`
  - The build job always uploads the executables as a workflow artifact (`codex-windows-binaries`) so the long build output isn't lost between runs.
  - A separate `release` job (tag pushes or manual dispatch with a `release_tag`) downloads that artifact and publishes it to a GitHub Release — no rebuild required to publish an already-built artifact.
- **All other workflows are disabled** — only `build-and-release.yml` runs on this repo. The other workflow files remain in the repo and can be re-enabled with `gh workflow enable <name>`.
- **Dependabot updates disabled** — the `updates:` list was removed from `.github/dependabot.yaml`, so no dependency update PRs are opened. Re-enable by restoring that list.

## Release history

Release tag convention in this fork is `v<original-version>-patched`. The first release is **`v0.144.0-patched`**, based on the `0.144.0` version referenced in `codex-rs/models-manager/models.json`.

Each release's GitHub Release contains two Windows executables:

- `codex.exe` — production (release) build
- `codex-dev.exe` — developer (debug) build

## Building

The build is a standard Cargo workspace build rooted at `codex-rs`:

```shell
# Developer (debug) build
cargo build --bin codex

# Production (release) build
cargo build --bin codex --release
```

All Rust work lives in `codex-rs`. Follow the repo conventions in `AGENTS.md` and the `justfile` for `fmt`, `fix`, and `test`.

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex) (upstream)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
