use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const NUM_TARGETS: usize = 5;
const PACKETS_PER_SEC: u64 = 100;
const TEST_DURATION_SECS: u64 = 5;

struct Game {
    name: &'static str,
    packet_size: usize,
}

const GAMES: &[Game] = &[
    Game {
        name: "Forza Motorsport",
        packet_size: 331,
    },
    Game {
        name: "ACC",
        packet_size: 608,
    },
    Game {
        name: "F1 24",
        packet_size: 1460,
    },
    Game {
        name: "LMU / rFactor 2",
        packet_size: 1684,
    },
    Game {
        name: "iRacing",
        packet_size: 2048,
    },
    Game {
        name: "Max UDP",
        packet_size: 8192,
    },
];

fn main() {
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .status()
        .expect("Failed to build");
    assert!(status.success(), "Build failed");

    let binary = std::env::current_dir()
        .unwrap()
        .join("target/release/udp-forwarder");

    println!("UDP Forwarder Smoke Test");
    println!("========================");
    println!("Binary: {}", binary.display());
    println!(
        "Targets: {}, Rate: {} pkt/s, Duration: {}s\n",
        NUM_TARGETS, PACKETS_PER_SEC, TEST_DURATION_SECS
    );

    let mut results = Vec::new();
    for (i, game) in GAMES.iter().enumerate() {
        let port_offset = (i as u16) * 200;
        let r = run_test(&binary, game, 19100 + port_offset, 19200 + port_offset);
        results.push(r);
    }

    // Print comparison table
    println!(
        "\n========== RESULTS ({} targets) ==========\n",
        NUM_TARGETS
    );

    // Header
    print!("{:<12}", "");
    for game in GAMES {
        print!("{:>16}", game.name);
    }
    println!();
    print!("{:<12}", "");
    for _ in GAMES {
        print!("{:>16}", "---");
    }
    println!();

    // Packet size row
    print!("{:<12}", "Bytes");
    for game in GAMES {
        print!("{:>16}", game.packet_size);
    }
    println!();

    // Delivery
    print!("{:<12}", "Delivery");
    for r in &results {
        print!("{:>15}%", format!("{:.1}", r.delivery_pct));
    }
    println!();

    // Latency rows
    let rows: Vec<(&str, Vec<f64>)> = vec![
        ("Avg", results.iter().map(|r| r.avg_us).collect()),
        ("P50", results.iter().map(|r| r.p50_us).collect()),
        ("P95", results.iter().map(|r| r.p95_us).collect()),
        ("P99", results.iter().map(|r| r.p99_us).collect()),
        ("Max", results.iter().map(|r| r.max_us).collect()),
    ];
    for (label, vals) in &rows {
        print!("{:<12}", label);
        for v in vals {
            print!("{:>14}", format!("{:.1}µs", v));
        }
        println!();
    }
}

struct TestResult {
    sent: u64,
    delivery_pct: f64,
    avg_us: f64,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
    max_us: f64,
}

fn write_config(
    path: &std::path::Path,
    listen_port: u16,
    num_targets: usize,
    target_port_base: u16,
) {
    let mut f = std::fs::File::create(path).expect("Failed to create config");
    writeln!(f, "[general]").unwrap();
    writeln!(f, "listen_port = {}", listen_port).unwrap();
    for i in 0..num_targets {
        writeln!(f, "\n[forward.{}]", i + 1).unwrap();
        writeln!(f, "ip = 127.0.0.1").unwrap();
        writeln!(f, "port = {}", target_port_base + i as u16).unwrap();
    }
}

fn start_forwarder(binary: &std::path::Path, config: &std::path::Path) -> Child {
    Command::new(binary)
        .arg(config)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start forwarder")
}

fn run_test(
    binary: &std::path::Path,
    game: &Game,
    listen_port: u16,
    target_port_base: u16,
) -> TestResult {
    println!("--- {} ({} bytes) ---", game.name, game.packet_size);

    let safe_name: String = game
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let config_path = std::env::temp_dir().join(format!("udp-bench-{}.ini", safe_name));
    write_config(&config_path, listen_port, NUM_TARGETS, target_port_base);

    let stop = Arc::new(AtomicBool::new(false));
    let recv_counts: Vec<Arc<AtomicU64>> = (0..NUM_TARGETS)
        .map(|_| Arc::new(AtomicU64::new(0)))
        .collect();

    // Start receivers
    let mut receiver_handles = Vec::new();
    for i in 0..NUM_TARGETS {
        let port = target_port_base + i as u16;
        let sock = UdpSocket::bind(format!("127.0.0.1:{}", port))
            .unwrap_or_else(|e| panic!("Failed to bind receiver on port {}: {}", port, e));
        sock.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let count = recv_counts[i].clone();
        let stop = stop.clone();
        receiver_handles.push(thread::spawn(move || {
            let mut buf = [0u8; 65535];
            while !stop.load(Ordering::Relaxed) {
                if sock.recv_from(&mut buf).is_ok() {
                    count.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    // Start forwarder
    let mut child = start_forwarder(binary, &config_path);
    thread::sleep(Duration::from_millis(500));

    // Throughput test
    let sender = UdpSocket::bind("0.0.0.0:0").unwrap();
    let dest: SocketAddr = format!("127.0.0.1:{}", listen_port).parse().unwrap();
    let payload = vec![0xABu8; game.packet_size];
    let interval = Duration::from_micros(1_000_000 / PACKETS_PER_SEC);
    let total_packets = PACKETS_PER_SEC * TEST_DURATION_SECS;

    let start = Instant::now();
    for i in 0..total_packets {
        sender.send_to(&payload, dest).unwrap();
        let expected = start + interval * (i as u32 + 1);
        let now = Instant::now();
        if now < expected {
            thread::sleep(expected - now);
        }
    }
    let elapsed = start.elapsed();

    thread::sleep(Duration::from_millis(1000));
    stop.store(true, Ordering::Relaxed);
    for h in receiver_handles {
        h.join().unwrap();
    }

    let total_received: u64 = recv_counts.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    let expected_total = total_packets * NUM_TARGETS as u64;
    let delivery_pct = total_received as f64 / expected_total as f64 * 100.0;

    println!(
        "  Throughput: {}/{} ({:.1}%) in {:.2}s",
        total_received,
        expected_total,
        delivery_pct,
        elapsed.as_secs_f64()
    );

    // Latency test — rebind receivers
    let stop2 = Arc::new(AtomicBool::new(false));
    let mut drain_handles = Vec::new();
    for i in 0..NUM_TARGETS - 1 {
        let port = target_port_base + i as u16;
        let sock = UdpSocket::bind(format!("127.0.0.1:{}", port)).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let stop = stop2.clone();
        drain_handles.push(thread::spawn(move || {
            let mut buf = [0u8; 65535];
            while !stop.load(Ordering::Relaxed) {
                let _ = sock.recv_from(&mut buf);
            }
        }));
    }

    let last_port = target_port_base + (NUM_TARGETS - 1) as u16;
    let last_recv = UdpSocket::bind(format!("127.0.0.1:{}", last_port)).unwrap();
    last_recv
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    let num_latency_packets: u64 = 1000;
    let mut latencies = Vec::with_capacity(num_latency_packets as usize);
    let mut buf = [0u8; 65535];

    for _ in 0..num_latency_packets {
        let t = Instant::now();
        sender.send_to(&payload, dest).unwrap();
        if last_recv.recv_from(&mut buf).is_ok() {
            latencies.push(t.elapsed());
        }
    }

    stop2.store(true, Ordering::Relaxed);
    for h in drain_handles {
        h.join().unwrap();
    }

    child.kill().ok();
    child.wait().ok();
    let _ = std::fs::remove_file(&config_path);

    if latencies.is_empty() {
        println!("  Latency: No packets received!");
        return TestResult {
            sent: total_packets,
            delivery_pct,
            avg_us: 0.0,
            p50_us: 0.0,
            p95_us: 0.0,
            p99_us: 0.0,
            max_us: 0.0,
        };
    }

    latencies.sort();
    let count = latencies.len();
    let avg = latencies.iter().map(|d| d.as_nanos()).sum::<u128>() / count as u128;
    let p50 = latencies[count / 2].as_nanos();
    let p95 = latencies[(count as f64 * 0.95) as usize].as_nanos();
    let p99 = latencies[(count as f64 * 0.99) as usize].as_nanos();
    let max = latencies[count - 1].as_nanos();

    println!(
        "  Latency: Avg {:.1}µs  P50 {:.1}µs  P95 {:.1}µs  P99 {:.1}µs  Max {:.1}µs",
        avg as f64 / 1000.0,
        p50 as f64 / 1000.0,
        p95 as f64 / 1000.0,
        p99 as f64 / 1000.0,
        max as f64 / 1000.0
    );

    TestResult {
        sent: total_packets,
        delivery_pct,
        avg_us: avg as f64 / 1000.0,
        p50_us: p50 as f64 / 1000.0,
        p95_us: p95 as f64 / 1000.0,
        p99_us: p99 as f64 / 1000.0,
        max_us: max as f64 / 1000.0,
    }
}
