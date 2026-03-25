use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const PACKETS_PER_SEC: u64 = 100;
const PACKET_SIZE: usize = 500;
const TEST_DURATION_SECS: u64 = 5;
const LISTEN_PORT: u16 = 19100;
const TARGET_PORT_BASE: u16 = 19200;

fn main() {
    // Build the release binary first
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
    println!("Packet size: {} bytes, Rate: {} pkt/s, Duration: {}s\n",
        PACKET_SIZE, PACKETS_PER_SEC, TEST_DURATION_SECS);

    let r10 = run_test(&binary, 10, LISTEN_PORT, TARGET_PORT_BASE);
    let r20 = run_test(&binary, 20, LISTEN_PORT + 100, TARGET_PORT_BASE + 100);
    let r100 = run_test(&binary, 100, LISTEN_PORT + 200, TARGET_PORT_BASE + 200);

    println!("\n========== RESULTS ==========\n");
    println!("{:<20} {:>12} {:>12} {:>12}", "", "10 targets", "20 targets", "100 targets");
    println!("{:<20} {:>12} {:>12} {:>12}", "---", "---", "---", "---");
    println!("{:<20} {:>12} {:>12} {:>12}", "Sent", r10.sent, r20.sent, r100.sent);
    println!("{:<20} {:>11}% {:>11}% {:>11}%", "Delivery",
        format!("{:.1}", r10.delivery_pct), format!("{:.1}", r20.delivery_pct), format!("{:.1}", r100.delivery_pct));
    for (label, v10, v20, v100) in [
        ("Avg", r10.avg_us, r20.avg_us, r100.avg_us),
        ("P50", r10.p50_us, r20.p50_us, r100.p50_us),
        ("P95", r10.p95_us, r20.p95_us, r100.p95_us),
        ("P99", r10.p99_us, r20.p99_us, r100.p99_us),
        ("Max", r10.max_us, r20.max_us, r100.max_us),
    ] {
        println!("{:<20} {:>10} {:>10} {:>10}", label,
            format!("{:.1}µs", v10), format!("{:.1}µs", v20), format!("{:.1}µs", v100));
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

/// Generate a config.ini for the given number of targets
fn write_config(path: &std::path::Path, listen_port: u16, num_targets: usize, target_port_base: u16) {
    let mut f = std::fs::File::create(path).expect("Failed to create config");
    writeln!(f, "[general]").unwrap();
    writeln!(f, "listen_port = {}", listen_port).unwrap();
    for i in 0..num_targets {
        writeln!(f, "\n[forward.{}]", i + 1).unwrap();
        writeln!(f, "ip = 127.0.0.1").unwrap();
        writeln!(f, "port = {}", target_port_base + i as u16).unwrap();
    }
}

/// Start the forwarder binary in headless mode
fn start_forwarder(binary: &std::path::Path, config: &std::path::Path) -> Child {
    Command::new(binary)
        .arg(config)
        .spawn()
        .expect("Failed to start forwarder")
}

fn run_test(
    binary: &std::path::Path,
    num_targets: usize,
    listen_port: u16,
    target_port_base: u16,
) -> TestResult {
    println!("--- {} targets ---", num_targets);

    // Write config
    let config_path = std::env::temp_dir().join(format!("udp-bench-{}.ini", num_targets));
    write_config(&config_path, listen_port, num_targets, target_port_base);

    // Start receivers
    let stop = Arc::new(AtomicBool::new(false));
    let recv_counts: Vec<Arc<AtomicU64>> = (0..num_targets)
        .map(|_| Arc::new(AtomicU64::new(0)))
        .collect();

    let mut receiver_handles = Vec::new();
    for i in 0..num_targets {
        let port = target_port_base + i as u16;
        let sock = UdpSocket::bind(format!("127.0.0.1:{}", port))
            .unwrap_or_else(|e| panic!("Failed to bind receiver on port {}: {}", port, e));
        sock.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
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

    // Start forwarder process
    let mut child = start_forwarder(binary, &config_path);

    // Wait for forwarder to bind
    thread::sleep(Duration::from_millis(500));

    // Latency measurement: send to last target only for timing
    let last_port = target_port_base + (num_targets - 1) as u16;
    let latency_sock = UdpSocket::bind(format!("127.0.0.1:{}", last_port + 1000)).unwrap();
    // We'll measure by timing send → first recv on any existing receiver

    // Send packets
    let sender = UdpSocket::bind("0.0.0.0:0").unwrap();
    let dest: SocketAddr = format!("127.0.0.1:{}", listen_port).parse().unwrap();
    let payload = vec![0xABu8; PACKET_SIZE];
    let interval = Duration::from_micros(1_000_000 / PACKETS_PER_SEC);
    let total_packets = PACKETS_PER_SEC * TEST_DURATION_SECS;

    // For latency: send individual packets and time round-trip via a dedicated receiver
    // But first do the throughput test
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

    // Wait for forwarding to complete
    thread::sleep(Duration::from_millis(1000));
    stop.store(true, Ordering::Relaxed);

    for h in receiver_handles {
        h.join().unwrap();
    }

    let total_received: u64 = recv_counts.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    let expected_total = total_packets * num_targets as u64;
    let delivery_pct = total_received as f64 / expected_total as f64 * 100.0;

    println!("  Throughput: Sent {} → Received {}/{} ({:.1}%) in {:.2}s",
        total_packets, total_received, expected_total, delivery_pct, elapsed.as_secs_f64());

    // Now latency test: restart receivers, send packets one at a time, measure
    let stop2 = Arc::new(AtomicBool::new(false));

    // Bind drain receivers for all but last
    let mut drain_handles = Vec::new();
    for i in 0..num_targets - 1 {
        let port = target_port_base + i as u16;
        let sock = UdpSocket::bind(format!("127.0.0.1:{}", port))
            .unwrap_or_else(|e| panic!("Rebind failed on port {}: {}", port, e));
        sock.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let stop = stop2.clone();
        drain_handles.push(thread::spawn(move || {
            let mut buf = [0u8; 65535];
            while !stop.load(Ordering::Relaxed) {
                let _ = sock.recv_from(&mut buf);
            }
        }));
    }

    // Last receiver for timing
    let last_recv = UdpSocket::bind(format!("127.0.0.1:{}", last_port))
        .expect("Failed to rebind last receiver");
    last_recv.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

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
    drop(latency_sock);

    // Kill forwarder
    child.kill().ok();
    child.wait().ok();
    let _ = std::fs::remove_file(&config_path);

    if latencies.is_empty() {
        println!("  Latency: No packets received!");
        return TestResult {
            sent: total_packets, delivery_pct,
            avg_us: 0.0, p50_us: 0.0, p95_us: 0.0, p99_us: 0.0, max_us: 0.0,
        };
    }

    latencies.sort();
    let count = latencies.len();
    let avg = latencies.iter().map(|d| d.as_nanos()).sum::<u128>() / count as u128;
    let p50 = latencies[count / 2].as_nanos();
    let p95 = latencies[(count as f64 * 0.95) as usize].as_nanos();
    let p99 = latencies[(count as f64 * 0.99) as usize].as_nanos();
    let max = latencies[count - 1].as_nanos();

    println!("  Latency: Avg {:.1}µs  P50 {:.1}µs  P95 {:.1}µs  P99 {:.1}µs  Max {:.1}µs  ({}/{} received)",
        avg as f64 / 1000.0, p50 as f64 / 1000.0, p95 as f64 / 1000.0,
        p99 as f64 / 1000.0, max as f64 / 1000.0, count, num_latency_packets);

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
