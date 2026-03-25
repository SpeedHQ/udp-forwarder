# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

UDP Forwarder — a cross-platform Rust app with a Slint GUI that receives UDP packets on a single port and forwards them to multiple configurable targets. Used for splitting game telemetry (e.g., Forza) to multiple consumers. Also supports headless mode via CLI.

## Build & Run

```bash
cargo build --release          # Release binary → target/release/udp-forwarder
cargo build                    # Debug build
cargo run                      # Run GUI mode
cargo run -- --headless        # Run headless with config.ini next to binary
cargo run -- path/to/config.ini  # Run headless with specific config
cargo run -- --version         # Print version
```

No test suite exists. The project has no linter configuration — use `cargo clippy` for linting and `cargo fmt` for formatting.

## Architecture

- **`src/main.rs`** — Single-file application. Contains config loading (INI), GUI setup with Slint callbacks, headless mode, and UDP forwarding logic. The forwarding runs in a background thread with `AtomicBool` stop flag and `AtomicU64` packet counter.
- **`ui/main.slint`** — Declarative UI. Defines `AppState` global (listen port, targets list, running state, callbacks) and `MainWindow` with `TargetRow` component. The `for` loop over `AppState.targets` renders unlimited forward targets dynamically.
- **`build.rs`** — Compiles `ui/main.slint` via `slint-build`. Slint types (`MainWindow`, `AppState`, `ForwardTarget`) are generated at build time and included via `slint::include_modules!()`.
- **`config.ini.example`** — INI format: `[general]` section with `listen_port`, then `[forward.N]` sections each with `ip` and `port`.

## Config Format

```ini
[general]
listen_port = 5300

[forward.1]
ip = 127.0.0.1
port = 5301
```

Any section starting with `forward` is treated as a target. The GUI allows adding/removing targets dynamically and saving to `config.ini`.

## CI/CD

GitHub Actions workflow (`.github/workflows/build.yml`) builds on tag push (`v*`) or manual dispatch. Matrix: Windows (x86_64-msvc), Linux (x86_64-gnu), macOS (aarch64-darwin). macOS builds are code-signed and notarized using Apple secrets stored in GitHub repository secrets. Release job creates a GitHub Release with zipped artifacts.

## Versioning

Version is in `Cargo.toml`. Use `bump-version.sh` to update, which patches `Cargo.toml` and the install scripts.
