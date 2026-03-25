use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PACKETS_PER_SEC: u64 = 100;
const PACKET_SIZE: usize = 500;
const TEST_DURATION_SECS: u64 = 5;
const SOCKET_BUF_SIZE: usize = 4 * 1024 * 1024;
const RING_CAPACITY: usize = 4096;

struct BroadcastRing {
    slots: Vec<Mutex<(usize, [u8; 65535])>>,
    head: AtomicUsize,
    capacity: usize,
}

impl BroadcastRing {
    fn new(capacity: usize) -> Self {
        let slots = (0..capacity)
            .map(|_| Mutex::new((0usize, [0u8; 65535])))
            .collect();
        Self { slots, head: AtomicUsize::new(0), capacity }
    }

    fn publish(&self, data: &[u8]) -> usize {
        let slot = self.head.fetch_add(1, Ordering::Release);
        let idx = slot % self.capacity;
        let mut buf = self.slots[idx].lock().unwrap();
        buf.0 = data.len();
        buf.1[..data.len()].copy_from_slice(data);
        slot
    }

    fn send_from(&self, slot: usize, sock: &UdpSocket) {
        let idx = slot % self.capacity;
        let buf = self.slots[idx].lock().unwrap();
        let _ = sock.send(&buf.1[..buf.0]);
    }
}

fn tune_socket(sock: &UdpSocket) {
    use std::os::fd::AsRawFd;
    let fd = sock.as_raw_fd();
    let size = SOCKET_BUF_SIZE as libc::c_int;
    unsafe {
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF,
            &size as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t);
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_SNDBUF,
            &size as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t);
    }
}

fn main() {
    println!("UDP Forwarding Benchmark");
    println!("========================\n");

    let r10 = run_throughput_bench(10, 19100, 19200);
    let r20 = run_throughput_bench(20, 19500, 19600);
    let r100 = run_throughput_bench(100, 20000, 20200);

    println!("\n========== COMPARISON ==========\n");
    println!("{:<25} {:>12} {:>12} {:>12}", "", "10 targets", "20 targets", "100 targets");
    println!("{:<25} {:>12} {:>12} {:>12}", "---", "---", "---", "---");
    println!("{:<25} {:>12} {:>12} {:>12}", "Sent", r10.sent, r20.sent, r100.sent);
    println!("{:<25} {:>12} {:>12} {:>12}", "Forwarded", r10.forwarded, r20.forwarded, r100.forwarded);
    println!("{:<25} {:>11}% {:>11}% {:>11}%", "Delivery",
        format!("{:.1}", r10.delivery_pct), format!("{:.1}", r20.delivery_pct), format!("{:.1}", r100.delivery_pct));
    println!("{:<25} {:>10} {:>10} {:>10}", "Throughput",
        format!("{:.2} MB/s", r10.throughput_mbps), format!("{:.2} MB/s", r20.throughput_mbps), format!("{:.2} MB/s", r100.throughput_mbps));

    println!("\nLatency comparison...\n");
    let l10 = run_latency_bench(10, 21000, 21100);
    let l20 = run_latency_bench(20, 21500, 21600);
    let l100 = run_latency_bench(100, 22000, 22200);

    println!("\n{:<25} {:>12} {:>12} {:>12}", "", "10 targets", "20 targets", "100 targets");
    println!("{:<25} {:>12} {:>12} {:>12}", "---", "---", "---", "---");
    for (label, v10, v20, v100) in [
        ("Avg", l10.avg_us, l20.avg_us, l100.avg_us),
        ("P50", l10.p50_us, l20.p50_us, l100.p50_us),
        ("P95", l10.p95_us, l20.p95_us, l100.p95_us),
        ("P99", l10.p99_us, l20.p99_us, l100.p99_us),
        ("Max", l10.max_us, l20.max_us, l100.max_us),
    ] {
        println!("{:<25} {:>10} {:>10} {:>10}", label,
            format!("{:.1}µs", v10), format!("{:.1}µs", v20), format!("{:.1}µs", v100));
    }
    println!("{:<25} {:>11}% {:>11}% {:>11}%", "Delivery",
        format!("{:.1}", l10.delivery_pct), format!("{:.1}", l20.delivery_pct), format!("{:.1}", l100.delivery_pct));
}

struct ThroughputResult { sent: u64, forwarded: u64, delivery_pct: f64, throughput_mbps: f64 }
struct LatencyResult { avg_us: f64, p50_us: f64, p95_us: f64, p99_us: f64, max_us: f64, delivery_pct: f64 }

fn run_throughput_bench(num_targets: usize, listen_port: u16, target_port_start: u16) -> ThroughputResult {
    println!("--- Throughput: {} targets, {} pkt/s, {} bytes ---", num_targets, PACKETS_PER_SEC, PACKET_SIZE);

    let stop = Arc::new(AtomicBool::new(false));
    let recv_counts: Vec<Arc<AtomicU64>> = (0..num_targets).map(|_| Arc::new(AtomicU64::new(0))).collect();

    let mut receiver_handles = Vec::new();
    for i in 0..num_targets {
        let port = target_port_start + i as u16;
        let sock = UdpSocket::bind(format!("127.0.0.1:{}", port))
            .unwrap_or_else(|e| panic!("Failed to bind receiver on port {}: {}", port, e));
        sock.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let count = recv_counts[i].clone();
        let stop = stop.clone();
        receiver_handles.push(thread::spawn(move || {
            let mut buf = [0u8; 65535];
            while !stop.load(Ordering::Relaxed) {
                if sock.recv_from(&mut buf).is_ok() { count.fetch_add(1, Ordering::Relaxed); }
            }
        }));
    }

    // Forwarder with broadcast ring
    let listen_sock = UdpSocket::bind(format!("127.0.0.1:{}", listen_port)).expect("Failed to bind forwarder");
    listen_sock.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    tune_socket(&listen_sock);

    let ring = Arc::new(BroadcastRing::new(RING_CAPACITY));
    let head = Arc::new(AtomicU64::new(0));

    for i in 0..num_targets {
        let target: SocketAddr = format!("127.0.0.1:{}", target_port_start + i as u16).parse().unwrap();
        let ring = ring.clone();
        let head = head.clone();
        let stop = stop.clone();
        let sock = UdpSocket::bind("0.0.0.0:0").unwrap();
        tune_socket(&sock);
        sock.connect(target).unwrap();
        thread::spawn(move || {
            let mut cursor: u64 = 0;
            loop {
                if stop.load(Ordering::Relaxed) { break; }
                let current = head.load(Ordering::Acquire);
                if cursor < current {
                    ring.send_from(cursor as usize, &sock);
                    cursor += 1;
                } else {
                    thread::yield_now();
                }
            }
        });
    }

    let fwd_stop = stop.clone();
    let fwd_count = Arc::new(AtomicU64::new(0));
    let fwd_count2 = fwd_count.clone();
    let fwd_ring = ring.clone();
    let fwd_head = head.clone();

    let forwarder = thread::spawn(move || {
        let mut buf = [0u8; 65535];
        while !fwd_stop.load(Ordering::Relaxed) {
            let (len, _) = match listen_sock.recv_from(&mut buf) {
                Ok(r) => r,
                Err(_) => continue,
            };
            fwd_ring.publish(&buf[..len]);
            fwd_head.fetch_add(1, Ordering::Release);
            fwd_count2.fetch_add(1, Ordering::Relaxed);
        }
    });

    // Send packets
    let sender = UdpSocket::bind("0.0.0.0:0").unwrap();
    let dest: SocketAddr = format!("127.0.0.1:{}", listen_port).parse().unwrap();
    let payload = vec![0xABu8; PACKET_SIZE];
    let interval = Duration::from_micros(1_000_000 / PACKETS_PER_SEC);
    let total_packets = PACKETS_PER_SEC * TEST_DURATION_SECS;

    let start = Instant::now();
    for i in 0..total_packets {
        sender.send_to(&payload, dest).unwrap();
        let expected = start + interval * (i as u32 + 1);
        let now = Instant::now();
        if now < expected { thread::sleep(expected - now); }
    }
    let elapsed = start.elapsed();

    thread::sleep(Duration::from_millis(500));
    stop.store(true, Ordering::Relaxed);
    for h in receiver_handles { h.join().unwrap(); }
    forwarder.join().unwrap();

    let forwarded = fwd_count.load(Ordering::Relaxed);
    let total_received: u64 = recv_counts.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    let expected_total = total_packets * num_targets as u64;
    let delivery_pct = total_received as f64 / expected_total as f64 * 100.0;
    let throughput_mbps = (total_packets as f64 * PACKET_SIZE as f64) / elapsed.as_secs_f64() / 1_000_000.0;

    println!("  Sent: {}  Forwarded: {}  Received: {}/{}  Delivery: {:.1}%  Throughput: {:.2} MB/s",
        total_packets, forwarded, total_received, expected_total, delivery_pct, throughput_mbps);

    ThroughputResult { sent: total_packets, forwarded, delivery_pct, throughput_mbps }
}

fn run_latency_bench(num_targets: usize, listen_port: u16, target_port_start: u16) -> LatencyResult {
    println!("--- Latency: {} targets, 1000 packets ---", num_targets);
    let num_packets: u64 = 1000;

    let stop = Arc::new(AtomicBool::new(false));

    // Bind all receiver sockets
    let recv_socks: Vec<UdpSocket> = (0..num_targets).map(|i| {
        let s = UdpSocket::bind(format!("127.0.0.1:{}", target_port_start + i as u16)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        s
    }).collect();

    // Drain all but the last target
    let mut drain_handles = Vec::new();
    for sock in recv_socks.into_iter().take(num_targets - 1) {
        let stop = stop.clone();
        drain_handles.push(thread::spawn(move || {
            let mut buf = [0u8; 65535];
            while !stop.load(Ordering::Relaxed) { let _ = sock.recv_from(&mut buf); }
        }));
    }

    let last_recv = UdpSocket::bind(format!("127.0.0.1:{}", target_port_start + (num_targets - 1) as u16)).unwrap();
    last_recv.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

    // Forwarder with broadcast ring
    let listen_sock = UdpSocket::bind(format!("127.0.0.1:{}", listen_port)).unwrap();
    listen_sock.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    tune_socket(&listen_sock);

    let ring = Arc::new(BroadcastRing::new(RING_CAPACITY));
    let head = Arc::new(AtomicU64::new(0));

    for i in 0..num_targets {
        let target: SocketAddr = format!("127.0.0.1:{}", target_port_start + i as u16).parse().unwrap();
        let ring = ring.clone();
        let head = head.clone();
        let stop = stop.clone();
        let sock = UdpSocket::bind("0.0.0.0:0").unwrap();
        tune_socket(&sock);
        sock.connect(target).unwrap();
        thread::spawn(move || {
            let mut cursor: u64 = 0;
            loop {
                if stop.load(Ordering::Relaxed) { break; }
                let current = head.load(Ordering::Acquire);
                if cursor < current {
                    ring.send_from(cursor as usize, &sock);
                    cursor += 1;
                } else {
                    thread::yield_now();
                }
            }
        });
    }

    let fwd_stop = stop.clone();
    let fwd_ring = ring.clone();
    let fwd_head = head.clone();
    let forwarder = thread::spawn(move || {
        let mut buf = [0u8; 65535];
        while !fwd_stop.load(Ordering::Relaxed) {
            let (len, _) = match listen_sock.recv_from(&mut buf) {
                Ok(r) => r,
                Err(_) => continue,
            };
            fwd_ring.publish(&buf[..len]);
            fwd_head.fetch_add(1, Ordering::Release);
        }
    });

    // Measure latency
    let sender = UdpSocket::bind("0.0.0.0:0").unwrap();
    let dest: SocketAddr = format!("127.0.0.1:{}", listen_port).parse().unwrap();
    let mut latencies = Vec::with_capacity(num_packets as usize);
    let mut buf = [0u8; 65535];

    for _ in 0..num_packets {
        let payload = [0u8; PACKET_SIZE];
        let t = Instant::now();
        sender.send_to(&payload, dest).unwrap();
        if last_recv.recv_from(&mut buf).is_ok() { latencies.push(t.elapsed()); }
    }

    stop.store(true, Ordering::Relaxed);
    for h in drain_handles { h.join().unwrap(); }
    forwarder.join().unwrap();

    if latencies.is_empty() {
        println!("  No packets received!");
        return LatencyResult { avg_us: 0.0, p50_us: 0.0, p95_us: 0.0, p99_us: 0.0, max_us: 0.0, delivery_pct: 0.0 };
    }

    latencies.sort();
    let count = latencies.len();
    let avg = latencies.iter().map(|d| d.as_nanos()).sum::<u128>() / count as u128;
    let p50 = latencies[count / 2].as_nanos();
    let p95 = latencies[(count as f64 * 0.95) as usize].as_nanos();
    let p99 = latencies[(count as f64 * 0.99) as usize].as_nanos();
    let max = latencies[count - 1].as_nanos();
    let delivery_pct = count as f64 / num_packets as f64 * 100.0;

    println!("  Avg: {:.1}µs  P50: {:.1}µs  P95: {:.1}µs  P99: {:.1}µs  Max: {:.1}µs  Delivery: {:.1}%",
        avg as f64 / 1000.0, p50 as f64 / 1000.0, p95 as f64 / 1000.0, p99 as f64 / 1000.0, max as f64 / 1000.0, delivery_pct);

    LatencyResult {
        avg_us: avg as f64 / 1000.0, p50_us: p50 as f64 / 1000.0, p95_us: p95 as f64 / 1000.0,
        p99_us: p99 as f64 / 1000.0, max_us: max as f64 / 1000.0, delivery_pct,
    }
}
