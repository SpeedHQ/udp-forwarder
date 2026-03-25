use ini::Ini;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::env;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

slint::include_modules!();

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SOCKET_BUF_SIZE: usize = 4 * 1024 * 1024; // 4MB socket buffer
const RING_CAPACITY: usize = 4096;

/// Pre-allocated broadcast ring buffer. Zero allocations on the hot path.
/// One writer publishes into pre-allocated slots, N readers copy out.
struct BroadcastRing {
    /// Pre-allocated fixed-size buffers — no heap allocation per packet
    slots: Vec<Mutex<(usize, [u8; 65535])>>,
    /// Monotonically increasing write cursor
    head: AtomicUsize,
    capacity: usize,
}

impl BroadcastRing {
    fn new(capacity: usize) -> Self {
        let slots = (0..capacity)
            .map(|_| Mutex::new((0usize, [0u8; 65535])))
            .collect();
        Self {
            slots,
            head: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Publish data into the next slot. Returns the slot index.
    /// Zero allocation — copies into pre-allocated buffer.
    fn publish(&self, data: &[u8]) -> usize {
        let slot = self.head.fetch_add(1, Ordering::Release);
        let idx = slot % self.capacity;
        let mut buf = self.slots[idx].lock().unwrap();
        buf.0 = data.len();
        buf.1[..data.len()].copy_from_slice(data);
        slot
    }

    /// Read data from a slot. Copies from pre-allocated buffer.
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
        libc::setsockopt(
            fd, libc::SOL_SOCKET, libc::SO_RCVBUF,
            &size as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
        libc::setsockopt(
            fd, libc::SOL_SOCKET, libc::SO_SNDBUF,
            &size as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

fn config_path() -> PathBuf {
    let arg_path = env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'));

    if let Some(path) = arg_path {
        return PathBuf::from(path);
    }

    let exe_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));

    exe_dir.join("config.ini")
}

struct Config {
    listen_port: u16,
    targets: Vec<(String, u16, String)>,
}

fn load_config(path: &PathBuf) -> Option<Config> {
    let conf = Ini::load_from_file(path).ok()?;
    let general = conf.section(Some("general"))?;
    let listen_port: u16 = general.get("listen_port")?.parse().ok()?;

    let mut targets = Vec::new();
    for (key, _) in conf.iter() {
        let section_name = match key {
            Some(name) if name.starts_with("forward") => name,
            _ => continue,
        };
        let section = conf.section(Some(section_name))?;
        let ip = section.get("ip")?;
        let port: u16 = section.get("port")?.parse().ok()?;
        let note = section.get("note").unwrap_or("").to_string();
        targets.push((ip.to_string(), port, note));
    }

    Some(Config { listen_port, targets })
}

fn save_config(path: &PathBuf, listen_port: &str, targets: &[(String, String, String)]) {
    let mut conf = Ini::new();
    conf.with_section(Some("general"))
        .set("listen_port", listen_port);

    for (i, (ip, port, note)) in targets.iter().enumerate() {
        let mut section = conf.with_section(Some(format!("forward.{}", i + 1)));
        section.set("ip", ip).set("port", port);
        if !note.is_empty() {
            section.set("note", note);
        }
    }

    if let Err(e) = conf.write_to_file(path) {
        eprintln!("Failed to save config: {}", e);
    }
}

/// Spawn sender threads for a set of targets using the broadcast ring.
/// Returns the ring, head counter, and stop flag for the sender threads.
fn spawn_ring_senders(
    targets: &[SocketAddr],
    ring: &Arc<BroadcastRing>,
    head: &Arc<AtomicU64>,
    stop: &Arc<AtomicBool>,
) {
    for target in targets {
        let target = *target;
        let ring = ring.clone();
        let head = head.clone();
        let stop = stop.clone();
        let sock = UdpSocket::bind("0.0.0.0:0").expect("Failed to create send socket");
        tune_socket(&sock);
        sock.connect(target).expect("Failed to connect send socket");

        thread::spawn(move || {
            let mut cursor: u64 = 0;
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
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
}

fn run_headless(config_path: PathBuf) {
    let config = load_config(&config_path).unwrap_or_else(|| {
        eprintln!("Failed to load config '{}'", config_path.display());
        std::process::exit(1);
    });

    if config.targets.is_empty() {
        eprintln!("No [forward.*] sections found in config");
        std::process::exit(1);
    }

    let targets: Vec<SocketAddr> = config
        .targets
        .iter()
        .map(|(ip, port, _note)| {
            format!("{}:{}", ip, port).parse().unwrap_or_else(|e| {
                eprintln!("Invalid address {}:{} — {}", ip, port, e);
                std::process::exit(1);
            })
        })
        .collect();

    let bind_addr = format!("0.0.0.0:{}", config.listen_port);
    let socket = UdpSocket::bind(&bind_addr).unwrap_or_else(|e| {
        eprintln!("Failed to bind to {}: {}", bind_addr, e);
        std::process::exit(1);
    });
    tune_socket(&socket);

    println!("Listening on UDP port {}", config.listen_port);
    for target in &targets {
        println!("  Forwarding to {}", target);
    }

    let ring = Arc::new(BroadcastRing::new(RING_CAPACITY));
    let head = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    spawn_ring_senders(&targets, &ring, &head, &stop);

    let mut buf = [0u8; 65535];
    loop {
        let (len, _src) = match socket.recv_from(&mut buf) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("recv error: {}", e);
                continue;
            }
        };

        ring.publish(&buf[..len]);
        head.fetch_add(1, Ordering::Release);
    }
}

fn main() {
    if env::args().any(|a| a == "--version" || a == "-v") {
        println!("udp-forwarder {}", VERSION);
        return;
    }

    // Headless mode: explicit flag or config path argument
    if env::args().any(|a| a == "--headless")
        || env::args().skip(1).any(|a| !a.starts_with('-'))
    {
        run_headless(config_path());
        return;
    }

    // --- GUI mode ---
    let main_window = MainWindow::new().unwrap();
    main_window.global::<AppState>().set_version(SharedString::from(format!("v{}", VERSION)));
    let config_file = config_path();

    // Load existing config into UI
    if let Some(config) = load_config(&config_file) {
        let state = main_window.global::<AppState>();
        state.set_listen_port(SharedString::from(config.listen_port.to_string()));

        let targets: Vec<ForwardTarget> = config
            .targets
            .iter()
            .map(|(ip, port, note)| ForwardTarget {
                ip: SharedString::from(ip.as_str()),
                port: SharedString::from(port.to_string()),
                note: SharedString::from(note.as_str()),
            })
            .collect();
        state.set_targets(ModelRc::new(VecModel::from(targets)));
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicU64::new(0));
    let thread_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>> = Arc::new(Mutex::new(None));

    // Add target
    {
        let w = main_window.as_weak();
        main_window.global::<AppState>().on_add_target(move || {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            let mut targets: Vec<ForwardTarget> = (0..model.row_count())
                .map(|i| model.row_data(i).unwrap())
                .collect();
            targets.push(ForwardTarget {
                ip: SharedString::from("127.0.0.1"),
                port: SharedString::from("5300"),
                note: SharedString::from(""),
            });
            state.set_targets(ModelRc::new(VecModel::from(targets)));
        });
    }

    // Remove target
    {
        let w = main_window.as_weak();
        main_window.global::<AppState>().on_remove_target(move |index| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            let mut targets: Vec<ForwardTarget> = (0..model.row_count())
                .map(|i| model.row_data(i).unwrap())
                .collect();
            if (index as usize) < targets.len() {
                targets.remove(index as usize);
            }
            state.set_targets(ModelRc::new(VecModel::from(targets)));
        });
    }

    // Update target IP
    {
        let w = main_window.as_weak();
        main_window.global::<AppState>().on_update_target_ip(move |index, value| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            let mut targets: Vec<ForwardTarget> = (0..model.row_count())
                .map(|i| model.row_data(i).unwrap())
                .collect();
            if let Some(target) = targets.get_mut(index as usize) {
                target.ip = value;
            }
            state.set_targets(ModelRc::new(VecModel::from(targets)));
        });
    }

    // Update target port
    {
        let w = main_window.as_weak();
        main_window.global::<AppState>().on_update_target_port(move |index, value| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            let mut targets: Vec<ForwardTarget> = (0..model.row_count())
                .map(|i| model.row_data(i).unwrap())
                .collect();
            if let Some(target) = targets.get_mut(index as usize) {
                target.port = value;
            }
            state.set_targets(ModelRc::new(VecModel::from(targets)));
        });
    }

    // Update target note
    {
        let w = main_window.as_weak();
        main_window.global::<AppState>().on_update_target_note(move |index, value| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            let mut targets: Vec<ForwardTarget> = (0..model.row_count())
                .map(|i| model.row_data(i).unwrap())
                .collect();
            if let Some(target) = targets.get_mut(index as usize) {
                target.note = value;
            }
            state.set_targets(ModelRc::new(VecModel::from(targets)));
        });
    }

    // Save config
    {
        let w = main_window.as_weak();
        let path = config_file.clone();
        let stop = stop_flag.clone();
        let handle = thread_handle.clone();
        main_window.global::<AppState>().on_save_config(move || {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let listen_port = state.get_listen_port().to_string();
            let model = state.get_targets();
            let targets: Vec<(String, String, String)> = (0..model.row_count())
                .map(|i| {
                    let t = model.row_data(i).unwrap();
                    (t.ip.to_string(), t.port.to_string(), t.note.to_string())
                })
                .collect();
            save_config(&path, &listen_port, &targets);
            state.set_status_text(SharedString::from("Config saved, restarting..."));
            // Stop current forwarder and wait for thread to release the port
            stop.store(true, Ordering::Relaxed);
            if let Some(h) = handle.lock().unwrap().take() {
                let _ = h.join();
            }
            state.set_running(false);
            state.invoke_start();
        });
    }

    // Start forwarder
    {
        let w = main_window.as_weak();
        let stop_flag = stop_flag.clone();
        let packet_count = packet_count.clone();
        let handle = thread_handle.clone();

        main_window.global::<AppState>().on_start(move || {
            let w_inner = w.upgrade().unwrap();
            let state = w_inner.global::<AppState>();

            let listen_port: u16 = match state.get_listen_port().to_string().parse() {
                Ok(p) => p,
                Err(_) => {
                    state.set_status_text(SharedString::from("Invalid listen port"));
                    return;
                }
            };

            let model = state.get_targets();
            let mut targets: Vec<SocketAddr> = Vec::new();
            for i in 0..model.row_count() {
                let t = model.row_data(i).unwrap();
                let addr_str = format!("{}:{}", t.ip, t.port);
                match addr_str.parse() {
                    Ok(addr) => targets.push(addr),
                    Err(_) => {
                        state.set_status_text(SharedString::from(format!("Invalid target: {}", addr_str)));
                        return;
                    }
                }
            }

            if targets.is_empty() {
                state.set_status_text(SharedString::from("Add at least one target"));
                return;
            }

            let bind_addr = format!("0.0.0.0:{}", listen_port);
            let socket = match UdpSocket::bind(&bind_addr) {
                Ok(s) => s,
                Err(e) => {
                    state.set_status_text(SharedString::from(format!("Bind failed: {}", e)));
                    return;
                }
            };

            socket.set_read_timeout(Some(std::time::Duration::from_millis(500))).ok();
            tune_socket(&socket);

            // Broadcast ring — zero allocation on hot path, same as headless
            let ring = Arc::new(BroadcastRing::new(RING_CAPACITY));
            let ring_head = Arc::new(AtomicU64::new(0));
            let sender_stop = Arc::new(AtomicBool::new(false));

            spawn_ring_senders(&targets, &ring, &ring_head, &sender_stop);

            stop_flag.store(false, Ordering::Relaxed);
            packet_count.store(0, Ordering::Relaxed);
            state.set_running(true);
            state.set_packets_forwarded(0);
            state.set_status_text(SharedString::from(format!("Listening on port {}", listen_port)));

            let stop = stop_flag.clone();
            let count = packet_count.clone();
            let w2 = w.clone();
            let handle = handle.clone();

            let h = thread::spawn(move || {
                let mut buf = [0u8; 65535];
                loop {
                    if stop.load(Ordering::Relaxed) {
                        sender_stop.store(true, Ordering::Relaxed);
                        break;
                    }

                    let (len, _src) = match socket.recv_from(&mut buf) {
                        Ok(result) => result,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => continue,
                        Err(_) => continue,
                    };

                    ring.publish(&buf[..len]);
                    ring_head.fetch_add(1, Ordering::Release);

                    let n = count.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 60 == 0 {
                        let _ = w2.upgrade_in_event_loop(move |main_window| {
                            let state = main_window.global::<AppState>();
                            state.set_packets_forwarded(n as i32);
                            state.set_status_text(SharedString::from(format!(
                                "Running — {} packets forwarded", n
                            )));
                        });
                    }
                }

                let _ = w2.upgrade_in_event_loop(move |main_window| {
                    let state = main_window.global::<AppState>();
                    state.set_running(false);
                    state.set_status_text(SharedString::from("Stopped"));
                });
            });
            *handle.lock().unwrap() = Some(h);
        });
    }

    // Stop forwarder
    {
        let stop_flag = stop_flag.clone();
        main_window.global::<AppState>().on_stop(move || {
            stop_flag.store(true, Ordering::Relaxed);
        });
    }

    // Auto-start forwarding if config has targets
    {
        let state = main_window.global::<AppState>();
        if state.get_targets().row_count() > 0 {
            state.invoke_start();
        }
    }

    main_window.run().unwrap();
}
