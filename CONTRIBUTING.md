# Contributing

## Build & Run

```bash
cargo build --release          # Release binary → target/release/udp-forwarder
cargo build                    # Debug build
cargo run                      # Run GUI mode
cargo run -- --headless        # Run headless with config.ini next to binary
cargo run -- path/to/config.ini  # Run headless with specific config
cargo run -- --version         # Print version
```

No test suite exists. Use `cargo clippy` for linting and `cargo fmt` for formatting.

## Architecture

- **`src/main.rs`** — Single-file application. Contains config loading (INI), GUI setup with Slint callbacks, headless mode, and UDP forwarding logic. Uses a `BroadcastRing` (lock-free ring buffer) for zero-allocation packet forwarding. Spawns one receiver thread per enabled listen port and one sender thread per destination, all sharing the same ring.
- **`ui/main.slint`** — Declarative UI with tabbed layout. Defines `AppState` global (listen ports, targets list, active tab, running state, callbacks) and `MainWindow` with `ListenPortRow` and `TargetRow` components. Two tabs: Listen (game port presets + custom ports) and Destinations (forwarding targets).
- **`build.rs`** — Compiles `ui/main.slint` via `slint-build`. Slint types (`MainWindow`, `AppState`, `ForwardTarget`, `ListenPort`) are generated at build time and included via `slint::include_modules!()`.
- **`config.ini.example`** — INI format: `[general]` section for app settings, `[listen.KEY]` sections for listen ports, `[forward.N]` sections for forwarding targets.

## Config Format

```ini
[general]
launch_on_startup = false
minimize_to_tray = false

[listen.f1]
port = 20777
enabled = true
label = F1 24

[listen.forza]
port = 4843
enabled = true
label = Forza Motorsport

[forward.1]
ip = 127.0.0.1
port = 5301
note = RaceIQ
```

Listen port presets (F1, Forza) are auto-populated and enabled by default. Custom listen ports can be added via the GUI. Any section starting with `forward` is treated as a destination target.

## CI/CD

- **Test** (`.github/workflows/test.yml`) — Runs on push to main and PRs, but only when code files change (`.rs`, `.slint`, `Cargo.toml`, `Cargo.lock`). Checks formatting, clippy, and build.
- **Build** (`.github/workflows/build.yml`) — Builds on tag push (`v*`) or manual dispatch. Matrix: Windows (x86_64-msvc), Linux (x86_64-gnu), macOS (aarch64-darwin). macOS builds are code-signed with `APPLE_SIGNING_IDENTITY` and notarized using `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID` secrets.
- **Release** (`.github/workflows/release.yml`) — Creates GitHub Release with artifacts and bumps version in `Cargo.toml` on main after release is published.

## Versioning

Version is in `Cargo.toml`. The release workflow automatically bumps the version after a release is published.
