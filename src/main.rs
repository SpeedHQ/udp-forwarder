use auto_launch::AutoLaunchBuilder;
use ini::Ini;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::env;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent, PredefinedMenuItem}};
use tray_icon::TrayIconEvent;

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
    targets: Vec<(String, String)>, // (address as "ip:port", note)
    launch_on_startup: bool,
    minimize_to_tray: bool,
}

fn load_config(path: &PathBuf) -> Option<Config> {
    let conf = Ini::load_from_file(path).ok()?;
    let general = conf.section(Some("general"))?;
    let listen_port: u16 = general.get("listen_port")?.parse().ok()?;
    let launch_on_startup = general.get("launch_on_startup").map_or(false, |v| v == "true");
    let minimize_to_tray = general.get("minimize_to_tray").map_or(false, |v| v == "true");

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
        targets.push((format!("{}:{}", ip, port), note));
    }

    Some(Config { listen_port, targets, launch_on_startup, minimize_to_tray })
}

fn save_config(path: &PathBuf, listen_port: &str, targets: &[(String, String)], launch_on_startup: bool, minimize_to_tray: bool) {
    let mut conf = Ini::new();
    conf.with_section(Some("general"))
        .set("listen_port", listen_port)
        .set("launch_on_startup", launch_on_startup.to_string())
        .set("minimize_to_tray", minimize_to_tray.to_string());

    for (i, (address, note)) in targets.iter().enumerate() {
        // Split address back into ip and port for INI compatibility
        let (ip, port) = address.rsplit_once(':').unwrap_or((address, "0"));
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

fn auto_launch() -> auto_launch::AutoLaunch {
    let exe = env::current_exe().unwrap_or_default();
    AutoLaunchBuilder::new()
        .set_app_name("UDP Forwarder")
        .set_app_path(exe.to_str().unwrap_or_default())
        .build()
        .expect("Failed to build AutoLaunch")
}

fn set_launch_on_startup(enabled: bool) {
    let launcher = auto_launch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    if let Err(e) = result {
        eprintln!("Failed to {} auto-launch: {}", if enabled { "enable" } else { "disable" }, e);
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
        .map(|(address, _note)| {
            use std::net::ToSocketAddrs;
            address.parse().unwrap_or_else(|_| {
                address.to_socket_addrs()
                    .ok()
                    .and_then(|mut addrs| addrs.next())
                    .unwrap_or_else(|| {
                        eprintln!("Invalid address: {}", address);
                        std::process::exit(1);
                    })
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

/// Validate a target address string. Returns empty string if valid, error message otherwise.
/// Accepts "host:port" where host can be an IP address, domain, or hostname.
fn validate_target_address(addr: &str) -> &'static str {
    if addr.is_empty() {
        return "";
    }
    if addr.starts_with("http://") || addr.starts_with("https://") {
        return "Remove http:// or https:// — enter host:port only";
    }
    let Some((host, port_str)) = addr.rsplit_once(':') else {
        return "Missing port — use host:port format (e.g. 127.0.0.1:5301)";
    };
    if host.is_empty() {
        return "Missing host address";
    }
    if port_str.is_empty() {
        return "Missing port number";
    }
    if port_str.parse::<u16>().is_err() {
        return "Invalid port number (must be 1-65535)";
    }
    // Validate host: IP address, domain, or hostname
    if host.parse::<std::net::Ipv4Addr>().is_err()
        && host.parse::<std::net::Ipv6Addr>().is_err()
        && !is_valid_hostname(host)
    {
        return "Invalid IP address or hostname";
    }
    ""
}

fn is_valid_hostname(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

/// Check all targets for duplicates and validation errors, update UI state accordingly.
fn refresh_target_errors(state: &AppState) {
    let model = state.get_targets();
    let count = model.row_count();

    // Collect addresses to detect duplicates
    let addresses: Vec<String> = (0..count)
        .map(|i| model.row_data(i).unwrap().address.to_string())
        .collect();

    let mut has_errors = false;
    for i in 0..count {
        if let Some(mut target) = model.row_data(i) {
            let addr = target.address.as_str();
            let validation = validate_target_address(addr);
            let is_dup = !addr.is_empty()
                && addresses.iter().enumerate().any(|(j, a)| j != i && a == addr);

            target.validation_error = SharedString::from(validation);
            target.is_duplicate = is_dup;
            model.set_row_data(i, target);

            if !validation.is_empty() || is_dup {
                has_errors = true;
            }
        }
    }

    state.set_has_errors(has_errors);
    if has_errors {
        let dup_count = (0..count)
            .filter(|&i| model.row_data(i).map_or(false, |t| t.is_duplicate))
            .count();
        let val_count = (0..count)
            .filter(|&i| model.row_data(i).map_or(false, |t| !t.validation_error.is_empty()))
            .count();
        let msg = match (dup_count > 0, val_count > 0) {
            (true, true) => "Duplicate and invalid targets found".to_string(),
            (true, false) => "Duplicate targets found".to_string(),
            (false, true) => format!("{} invalid target{}", val_count, if val_count > 1 { "s" } else { "" }),
            _ => String::new(),
        };
        state.set_error_text(SharedString::from(msg));
    } else {
        state.set_error_text(SharedString::default());
    }
}

#[derive(Clone)]
struct SavedState {
    listen_port: String,
    targets: Vec<(String, String)>, // (address, note)
}

impl SavedState {
    fn from_ui(state: &AppState) -> Self {
        let model = state.get_targets();
        let targets = (0..model.row_count())
            .map(|i| {
                let t = model.row_data(i).unwrap();
                (t.address.to_string(), t.note.to_string())
            })
            .collect();
        Self {
            listen_port: state.get_listen_port().to_string(),
            targets,
        }
    }

    fn matches_ui(&self, state: &AppState) -> bool {
        if self.listen_port != state.get_listen_port().as_str() {
            return false;
        }
        let model = state.get_targets();
        if self.targets.len() != model.row_count() {
            return false;
        }
        for (i, (addr, note)) in self.targets.iter().enumerate() {
            let t = model.row_data(i).unwrap();
            if addr != t.address.as_str() || note != t.note.as_str() {
                return false;
            }
        }
        true
    }
}

fn check_pending_changes(state: &AppState, saved: &SavedState) {
    state.set_has_pending_changes(!saved.matches_ui(state));
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

    // Set macOS dock icon (winit doesn't support this, must use NSApplication directly)
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2::AllocAnyThread;
        let icon_data = include_bytes!(concat!(env!("OUT_DIR"), "/app_icon.png"));
        let data = objc2_foundation::NSData::with_bytes(icon_data);
        if let Some(ns_image) = objc2_app_kit::NSImage::initWithData(objc2_app_kit::NSImage::alloc(), &data) {
            let app = objc2_app_kit::NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
            unsafe { app.setApplicationIconImage(Some(&ns_image)) };
        }
    }

    main_window.global::<AppState>().set_version(SharedString::from(format!("v{}", VERSION)));
    main_window.global::<AppState>().on_open_github(|| {
        let _ = open::that("https://github.com/SpeedHQ/udp-forwarder");
    });
    let config_file = config_path();

    // Load existing config into UI
    if let Some(config) = load_config(&config_file) {
        let state = main_window.global::<AppState>();
        state.set_listen_port(SharedString::from(config.listen_port.to_string()));

        let targets: Vec<ForwardTarget> = config
            .targets
            .iter()
            .map(|(address, note)| ForwardTarget {
                address: SharedString::from(address.as_str()),
                note: SharedString::from(note.as_str()),
                validation_error: SharedString::default(),
                is_duplicate: false,
            })
            .collect();
        state.set_targets(ModelRc::new(VecModel::from(targets)));
        state.set_launch_on_startup(config.launch_on_startup);
        state.set_minimize_to_tray(config.minimize_to_tray);
    }

    let saved_state = Arc::new(Mutex::new(SavedState::from_ui(&main_window.global::<AppState>())));

    let stop_flag = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicU64::new(0));
    let thread_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>> = Arc::new(Mutex::new(None));

    // Update listen port
    {
        let w = main_window.as_weak();
        let saved = saved_state.clone();
        main_window.global::<AppState>().on_update_listen_port(move |value| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            state.set_listen_port(value);
            check_pending_changes(&state, &saved.lock().unwrap());
        });
    }

    // Add target
    {
        let w = main_window.as_weak();
        let saved = saved_state.clone();
        main_window.global::<AppState>().on_add_target(move || {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            let mut targets: Vec<ForwardTarget> = (0..model.row_count())
                .map(|i| model.row_data(i).unwrap())
                .collect();
            targets.push(ForwardTarget {
                address: SharedString::from("127.0.0.1:5300"),
                note: SharedString::from(""),
                validation_error: SharedString::default(),
                is_duplicate: false,
            });
            state.set_targets(ModelRc::new(VecModel::from(targets)));
            refresh_target_errors(&state);
            check_pending_changes(&state, &saved.lock().unwrap());
        });
    }

    // Remove target
    {
        let w = main_window.as_weak();
        let saved = saved_state.clone();
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
            refresh_target_errors(&state);
            check_pending_changes(&state, &saved.lock().unwrap());
        });
    }

    // Update target address with validation (in-place to preserve focus)
    {
        let w = main_window.as_weak();
        let saved = saved_state.clone();
        main_window.global::<AppState>().on_update_target_address(move |index, value| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            if let Some(mut target) = model.row_data(index as usize) {
                target.address = value;
                model.set_row_data(index as usize, target);
            }
            refresh_target_errors(&state);
            check_pending_changes(&state, &saved.lock().unwrap());
        });
    }

    // Update target note (in-place to preserve focus)
    {
        let w = main_window.as_weak();
        let saved = saved_state.clone();
        main_window.global::<AppState>().on_update_target_note(move |index, value| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let model = state.get_targets();
            if let Some(mut target) = model.row_data(index as usize) {
                target.note = value;
                model.set_row_data(index as usize, target);
            }
            check_pending_changes(&state, &saved.lock().unwrap());
        });
    }

    // Cancel changes — reload config from disk
    {
        let w = main_window.as_weak();
        let path = config_file.clone();
        main_window.global::<AppState>().on_cancel_changes(move || {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            if let Some(config) = load_config(&path) {
                state.set_listen_port(SharedString::from(config.listen_port.to_string()));
                let targets: Vec<ForwardTarget> = config
                    .targets
                    .iter()
                    .map(|(address, note)| ForwardTarget {
                        address: SharedString::from(address.as_str()),
                        note: SharedString::from(note.as_str()),
                        validation_error: SharedString::default(),
                        is_duplicate: false,
                    })
                    .collect();
                state.set_targets(ModelRc::new(VecModel::from(targets)));
            }
            state.set_has_pending_changes(false);
            state.set_has_errors(false);
            state.set_error_text(SharedString::default());
        });
    }

    // Toggle launch on startup
    {
        let w = main_window.as_weak();
        let path = config_file.clone();
        main_window.global::<AppState>().on_toggle_launch_on_startup(move |enabled| {
            set_launch_on_startup(enabled);
            // Persist to config
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let listen_port = state.get_listen_port().to_string();
            let model = state.get_targets();
            let targets: Vec<(String, String)> = (0..model.row_count())
                .map(|i| {
                    let t = model.row_data(i).unwrap();
                    (t.address.to_string(), t.note.to_string())
                })
                .collect();
            save_config(&path, &listen_port, &targets, enabled, state.get_minimize_to_tray());
        });
    }

    // Toggle minimize to tray
    {
        let w = main_window.as_weak();
        let path = config_file.clone();
        main_window.global::<AppState>().on_toggle_minimize_to_tray(move |enabled| {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let listen_port = state.get_listen_port().to_string();
            let model = state.get_targets();
            let targets: Vec<(String, String)> = (0..model.row_count())
                .map(|i| {
                    let t = model.row_data(i).unwrap();
                    (t.address.to_string(), t.note.to_string())
                })
                .collect();
            save_config(&path, &listen_port, &targets, state.get_launch_on_startup(), enabled);
        });
    }

    // Save config
    {
        let w = main_window.as_weak();
        let path = config_file.clone();
        let stop = stop_flag.clone();
        let handle = thread_handle.clone();
        let saved = saved_state.clone();
        main_window.global::<AppState>().on_save_config(move || {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let listen_port = state.get_listen_port().to_string();
            let model = state.get_targets();
            let targets: Vec<(String, String)> = (0..model.row_count())
                .map(|i| {
                    let t = model.row_data(i).unwrap();
                    (t.address.to_string(), t.note.to_string())
                })
                .collect();
            let launch_on_startup = state.get_launch_on_startup();
            save_config(&path, &listen_port, &targets, launch_on_startup, state.get_minimize_to_tray());
            *saved.lock().unwrap() = SavedState::from_ui(&state);
            state.set_has_pending_changes(false);
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
                let addr_str = t.address.to_string();
                // Try direct parse first (works for IP:port), then DNS resolve for hostnames
                match addr_str.parse() {
                    Ok(addr) => targets.push(addr),
                    Err(_) => {
                        use std::net::ToSocketAddrs;
                        match addr_str.to_socket_addrs() {
                            Ok(mut addrs) => {
                                if let Some(addr) = addrs.next() {
                                    targets.push(addr);
                                } else {
                                    state.set_status_text(SharedString::from(format!("Could not resolve: {}", addr_str)));
                                    return;
                                }
                            }
                            Err(_) => {
                                state.set_status_text(SharedString::from(format!("Invalid target: {}", addr_str)));
                                return;
                            }
                        }
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
                let mut last_ui_update = Instant::now();
                let mut last_pps_count: u64 = 0;
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
                    let elapsed = last_ui_update.elapsed();
                    if elapsed.as_millis() >= 1000 {
                        let pps = ((n - last_pps_count) as f64 / elapsed.as_secs_f64()) as i32;
                        last_pps_count = n;
                        last_ui_update = Instant::now();
                        let _ = w2.upgrade_in_event_loop(move |main_window| {
                            let state = main_window.global::<AppState>();
                            state.set_packets_forwarded(n as i32);
                            state.set_packets_per_second(pps);
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

    // --- System tray ---
    let icon_bytes = include_bytes!(concat!(env!("OUT_DIR"), "/tray_icon.png"));
    let icon_img = image::load_from_memory(icon_bytes).expect("Failed to load tray icon").into_rgba8();
    let (w_icon, h_icon) = icon_img.dimensions();
    let tray_icon_data = tray_icon::Icon::from_rgba(icon_img.into_raw(), w_icon, h_icon)
        .expect("Failed to create tray icon");

    let status_item = MenuItem::new("Stopped", false, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let tray_menu = Menu::new();
    tray_menu.append_items(&[&status_item, &PredefinedMenuItem::separator(), &quit_item]).unwrap();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("UDP Forwarder")
        .with_icon(tray_icon_data)
        .with_menu_on_left_click(false)
        .build()
        .expect("Failed to create tray icon");

    let quit_item_id = quit_item.id().clone();

    // Poll tray events via Slint timer
    {
        let w = main_window.as_weak();
        let quit_id = quit_item_id.clone();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
            // Handle menu events (Quit)
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == quit_id {
                    slint::quit_event_loop().ok();
                }
            }
            // Handle tray icon click (show window)
            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                if matches!(event, TrayIconEvent::Click { .. }) {
                    if let Some(w) = w.upgrade() {
                        w.window().show().ok();
                    }
                }
            }
        });
        // Leak the timer so it lives for the duration of the app
        std::mem::forget(timer);
    }

    // Handle window close — hide to tray or quit
    {
        let w = main_window.as_weak();
        main_window.window().on_close_requested(move || {
            if let Some(w) = w.upgrade() {
                if w.global::<AppState>().get_minimize_to_tray() {
                    w.window().hide().ok();
                    return slint::CloseRequestResponse::KeepWindowShown;
                }
            }
            slint::quit_event_loop().ok();
            slint::CloseRequestResponse::HideWindow
        });
    }

    // Timer to update tray status text from UI state
    {
        let w = main_window.as_weak();
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), move || {
            if let Some(w) = w.upgrade() {
                let state = w.global::<AppState>();
                let text = if state.get_running() {
                    format!("Running — {} pkt/s", state.get_packets_per_second())
                } else {
                    "Stopped".to_string()
                };
                status_item.set_text(&text);
            }
        });
        std::mem::forget(timer);
    }

    main_window.show().unwrap();
    slint::run_event_loop_until_quit().unwrap();
}
