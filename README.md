# UDP Forwarder

A lightweight UDP packet forwarder written in Rust. Listens on a single port and forwards all received packets to multiple destinations. Built as a companion to [RaceIQ](https://github.com/SpeedHQ/RaceIQ) for splitting Forza's UDP output to multiple consumers.

## Quick Start

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/SpeedHQ/udp-forwarder/main/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/SpeedHQ/udp-forwarder/main/install.ps1 | iex
```

Both installers download the latest release, install the binary, and create a `config.ini` in your current directory.

## Configuration

Edit `config.ini` to set the listen port and forwarding targets:

```ini
[general]
listen_port = 5300

[forward.1]
ip = 127.0.0.1
port = 5301

[forward.2]
ip = 192.168.1.100
port = 5300
```

- `[general]` — `listen_port` is the UDP port to receive packets on
- `[forward.*]` — any section starting with `forward` defines a target. Each needs `ip` and `port`

## Running

```bash
udp-forwarder                # uses config.ini next to the binary
udp-forwarder my-config.ini  # custom config path
udp-forwarder --version      # print version
```

On Windows, use `udp-forwarder.exe` or double-click the executable.

## Manual Install

If you prefer not to use the install script, grab a zip from [Releases](https://github.com/SpeedHQ/udp-forwarder/releases):

| Platform | File |
|----------|------|
| Windows (x86_64) | `udp-forwarder-v*-x86_64-pc-windows-msvc.zip` |
| Linux (x86_64) | `udp-forwarder-v*-x86_64-unknown-linux-gnu.zip` |
| macOS (Apple Silicon) | `udp-forwarder-v*-aarch64-apple-darwin.zip` |

Each zip contains the binary and a default `config.ini`.

**macOS note:** Remove the quarantine attribute after extracting: `xattr -d com.apple.quarantine udp-forwarder`

## Building from Source

```bash
cargo build --release
```

Binary outputs to `target/release/udp-forwarder`.

## Versioning

Version is tracked in `Cargo.toml` and embedded in the binary.

```bash
./bump-version.sh patch   # 0.1.0 → 0.1.1
./bump-version.sh minor   # 0.1.0 → 0.2.0
./bump-version.sh major   # 0.1.0 → 1.0.0
```

## Releasing

1. Bump the version: `./bump-version.sh patch`
2. Commit the change
3. Tag: `git tag v<version>`
4. Push: `git push && git push --tags`

The GitHub Actions workflow builds for Windows (x86_64), Linux (x86_64), and macOS (ARM), then creates a GitHub Release with zipped artifacts containing the binary and a default `config.ini`.
