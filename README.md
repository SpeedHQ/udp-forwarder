# UDP Forwarder

A lightweight UDP packet forwarder written in Rust. Listens on a single port and forwards all received packets to multiple destinations. Built as a companion to [RaceIQ](https://github.com/SpeedHQ/RaceIQ) for splitting Forza's UDP output to multiple consumers.

## Download

Grab the latest release for your platform from [Releases](https://github.com/SpeedHQ/udp-forwarder/releases):

| Platform | File |
|----------|------|
| Windows (x86_64) | `udp-forwarder-v*-x86_64-pc-windows-msvc.zip` |
| Linux (x86_64) | `udp-forwarder-v*-x86_64-unknown-linux-gnu.zip` |
| macOS (Apple Silicon) | `udp-forwarder-v*-aarch64-apple-darwin.zip` |

Each zip contains the binary and a default `config.ini`.

## Usage

### Windows

1. Extract the zip
2. Edit `config.ini` (see [Configuration](#configuration))
3. Double-click `udp-forwarder.exe` or run from a terminal:

```powershell
.\udp-forwarder.exe                # uses config.ini next to the binary
.\udp-forwarder.exe my-config.ini  # custom config path
.\udp-forwarder.exe --version      # print version
```

### macOS

1. Extract the zip
2. Remove the quarantine attribute (unsigned binary):

```bash
xattr -d com.apple.quarantine udp-forwarder
```

3. Edit `config.ini` (see [Configuration](#configuration))
4. Run:

```bash
./udp-forwarder                # uses config.ini next to the binary
./udp-forwarder my-config.ini  # custom config path
./udp-forwarder --version      # print version
```

### Linux

1. Extract the zip
2. Make executable and run:

```bash
chmod +x udp-forwarder
./udp-forwarder                # uses config.ini next to the binary
./udp-forwarder my-config.ini  # custom config path
./udp-forwarder --version      # print version
```

## Configuration

Create a `config.ini` file (see `config.ini.example`):

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

## Building

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
