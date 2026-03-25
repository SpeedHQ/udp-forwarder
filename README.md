# UDP Forwarder

A lightweight UDP packet forwarder written in Rust. Listens on a single port and forwards all received packets to multiple destinations — useful for splitting game telemetry or any UDP stream to multiple consumers. Built as an optional companion to [RaceIQ](https://github.com/SpeedHQ/RaceIQ) so sim racers can receive telemetry on all their gear and software simultaneously with minimum latency.

## Quick Start

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/SpeedHQ/udp-forwarder/main/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/SpeedHQ/udp-forwarder/main/install.ps1 | iex
```

Both installers download the latest release and install the binary.

## GUI Mode

Launch the app to configure everything visually — set the listen port, add or remove forwarding targets, and start/stop forwarding. Settings are saved automatically to `config.ini`.

```bash
udp-forwarder
```

On Windows, use `udp-forwarder.exe` or double-click the executable.

## CLI Mode (Headless)

For servers or automation, run without the GUI:

```bash
udp-forwarder --headless              # uses config.ini next to the binary
udp-forwarder path/to/config.ini      # custom config path
udp-forwarder --version               # print version
```

### Config Format

Create a `config.ini`:

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

## Manual Install

If you prefer not to use the install script, grab a zip from [Releases](https://github.com/SpeedHQ/udp-forwarder/releases):

| Platform | File |
|----------|------|
| Windows (x86_64) | `udp-forwarder-v*-x86_64-pc-windows-msvc.zip` |
| Linux (x86_64) | `udp-forwarder-v*-x86_64-unknown-linux-gnu.zip` |
| macOS (Apple Silicon) | `udp-forwarder-v*-aarch64-apple-darwin.zip` |

Each zip contains the binary and a default `config.ini`.

**macOS note:** Remove the quarantine attribute after extracting: `xattr -d com.apple.quarantine udp-forwarder`

## Performance

Architecture uses parallel fan-out with one dedicated sender thread per target, pre-allocated broadcast ring buffer, connected UDP sockets, and 4MB socket buffers. Zero heap allocations on the hot path.

Run the benchmark: `cargo bench --bench forwarding_bench`

The benchmark is a black-box smoke test that spawns the actual binary in headless mode, sends real packets, and measures delivery and latency on external receivers.

**Per-game latency** (5 targets at 100 pkt/s, tested on M5 MacBook Pro):

| | Forza Motorsport | ACC | F1 24 | LMU / rFactor 2 | iRacing | Max UDP |
|---|---|---|---|---|---|---|
| Packet size | 331 bytes | 608 bytes | 1460 bytes | 1684 bytes | 2048 bytes | 8192 bytes |
| Avg | 68µs | 67µs | 53µs | 63µs | 67µs | 71µs |
| P50 | 64µs | 64µs | 51µs | 60µs | 63µs | 70µs |
| P95 | 109µs | 106µs | 84µs | 100µs | 107µs | 111µs |
| P99 | 128µs | 129µs | 99µs | 120µs | 124µs | 135µs |
| Max | 158µs | 150µs | 137µs | 181µs | 145µs | 147µs |
| Delivery | 100% | 100% | 100% | 100% | 100% | 100% |

Zero packet loss across all games. Sub-135µs P99 even at 8KB packets.

## Building from Source

```bash
cargo build --release
```

Binary outputs to `target/release/udp-forwarder`.

