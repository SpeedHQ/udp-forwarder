use ini::Ini;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::env;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

slint::include_modules!();

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    targets: Vec<(String, u16)>,
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
        targets.push((ip.to_string(), port));
    }

    Some(Config { listen_port, targets })
}

fn save_config(path: &PathBuf, listen_port: &str, targets: &[(String, String)]) {
    let mut conf = Ini::new();
    conf.with_section(Some("general"))
        .set("listen_port", listen_port);

    for (i, (ip, port)) in targets.iter().enumerate() {
        conf.with_section(Some(format!("forward.{}", i + 1)))
            .set("ip", ip)
            .set("port", port);
    }

    if let Err(e) = conf.write_to_file(path) {
        eprintln!("Failed to save config: {}", e);
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
        .map(|(ip, port)| {
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

    println!("Listening on UDP port {}", config.listen_port);
    for target in &targets {
        println!("  Forwarding to {}", target);
    }

    let mut buf = [0u8; 65535];
    loop {
        let (len, _src) = match socket.recv_from(&mut buf) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("recv error: {}", e);
                continue;
            }
        };

        let data = &buf[..len];
        for target in &targets {
            if let Err(e) = socket.send_to(data, target) {
                eprintln!("Failed to forward to {}: {}", target, e);
            }
        }
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
    let config_file = config_path();

    // Load existing config into UI
    if let Some(config) = load_config(&config_file) {
        let state = main_window.global::<AppState>();
        state.set_listen_port(SharedString::from(config.listen_port.to_string()));

        let targets: Vec<ForwardTarget> = config
            .targets
            .iter()
            .map(|(ip, port)| ForwardTarget {
                ip: SharedString::from(ip.as_str()),
                port: SharedString::from(port.to_string()),
            })
            .collect();
        state.set_targets(ModelRc::new(VecModel::from(targets)));
    }

    let stop_flag = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicU64::new(0));

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

    // Save config
    {
        let w = main_window.as_weak();
        let path = config_file.clone();
        main_window.global::<AppState>().on_save_config(move || {
            let w = w.upgrade().unwrap();
            let state = w.global::<AppState>();
            let listen_port = state.get_listen_port().to_string();
            let model = state.get_targets();
            let targets: Vec<(String, String)> = (0..model.row_count())
                .map(|i| {
                    let t = model.row_data(i).unwrap();
                    (t.ip.to_string(), t.port.to_string())
                })
                .collect();
            save_config(&path, &listen_port, &targets);
            state.set_status_text(SharedString::from("Config saved"));
        });
    }

    // Start forwarder
    {
        let w = main_window.as_weak();
        let stop_flag = stop_flag.clone();
        let packet_count = packet_count.clone();

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

            stop_flag.store(false, Ordering::Relaxed);
            packet_count.store(0, Ordering::Relaxed);
            state.set_running(true);
            state.set_packets_forwarded(0);
            state.set_status_text(SharedString::from(format!("Listening on port {}", listen_port)));

            let stop = stop_flag.clone();
            let count = packet_count.clone();
            let w2 = w.clone();

            thread::spawn(move || {
                let mut buf = [0u8; 65535];
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }

                    let (len, _src) = match socket.recv_from(&mut buf) {
                        Ok(result) => result,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => continue,
                        Err(_) => continue,
                    };

                    let data = &buf[..len];
                    for target in &targets {
                        let _ = socket.send_to(data, target);
                    }

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
        });
    }

    // Stop forwarder
    {
        let stop_flag = stop_flag.clone();
        main_window.global::<AppState>().on_stop(move || {
            stop_flag.store(true, Ordering::Relaxed);
        });
    }

    main_window.run().unwrap();
}
