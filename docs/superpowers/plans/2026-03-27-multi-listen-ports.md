# Multi-Listen-Port with Tabbed UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace single listen port with multiple listen ports (pre-populated with known game ports), add tabbed UI separating Listen and Destinations panels, and bind all enabled ports simultaneously forwarding to the same destinations.

**Architecture:** Config changes from single `listen_port` to multiple `[listen.*]` sections with per-port enable/disable. GUI gets two tabs (Listen | Destinations) with the middle content area switching between them while header and status bar remain fixed. Core spawns one receiver thread per enabled listen port, all feeding the shared BroadcastRing.

**Tech Stack:** Rust, Slint UI, rust-ini, existing BroadcastRing

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `ui/main.slint` | Modify | Add ListenPort struct, tab switching, Listen tab with preset rows + custom add, move destinations into Destinations tab |
| `src/main.rs` | Modify | Update Config struct, load/save for multi-port, update AppState bindings, spawn multiple receiver threads |
| `config.ini.example` | Modify | Update to show new multi-port format |

---

### Task 1: Update Config Struct and Parsing

**Files:**
- Modify: `src/main.rs:131-192` (Config struct, load_config, save_config)

- [ ] **Step 1: Update Config struct**

Replace `listen_port: u16` with `listen_ports: Vec<ListenPortConfig>`:

```rust
#[derive(Clone)]
struct ListenPortConfig {
    key: String,        // e.g. "raceiq", "f1", "forza", "custom_1"
    port: u16,
    enabled: bool,
    label: String,      // e.g. "RaceIQ", "F1 24", "Forza Motorsport"
    is_preset: bool,    // presets can't be removed, only toggled
}

struct Config {
    listen_ports: Vec<ListenPortConfig>,
    targets: Vec<(String, String)>,
    launch_on_startup: bool,
    minimize_to_tray: bool,
}
```

- [ ] **Step 2: Define preset ports constant**

```rust
const PRESET_LISTEN_PORTS: &[(&str, u16, &str)] = &[
    ("raceiq", 9301, "RaceIQ"),
    ("f1", 20888, "F1 24"),
    ("forza", 4843, "Forza Motorsport"),
];
```

- [ ] **Step 3: Update load_config**

Parse `[listen.*]` sections. If no listen sections found, fall back to legacy `listen_port` from `[general]` for backwards compatibility:

```rust
fn load_config(path: &PathBuf) -> Option<Config> {
    let conf = Ini::load_from_file(path).ok()?;
    let general = conf.section(Some("general"))?;
    let launch_on_startup = general.get("launch_on_startup") == Some("true");
    let minimize_to_tray = general.get("minimize_to_tray") == Some("true");

    // Parse listen ports
    let mut listen_ports: Vec<ListenPortConfig> = Vec::new();
    for (key, _) in conf.iter() {
        let section_name = match key {
            Some(name) if name.starts_with("listen.") => name,
            _ => continue,
        };
        let section = conf.section(Some(section_name))?;
        let port_key = section_name.strip_prefix("listen.").unwrap().to_string();
        let port: u16 = section.get("port")?.parse().ok()?;
        let enabled = section.get("enabled") != Some("false");
        let label = section.get("label").unwrap_or("").to_string();
        let is_preset = PRESET_LISTEN_PORTS.iter().any(|(k, _, _)| *k == port_key);
        listen_ports.push(ListenPortConfig {
            key: port_key,
            port,
            enabled,
            label: if label.is_empty() && is_preset {
                PRESET_LISTEN_PORTS.iter().find(|(k,_,_)| *k == listen_ports.last().unwrap().key).map(|(_,_,l)| l.to_string()).unwrap_or_default()
            } else {
                label
            },
            is_preset,
        });
    }

    // Legacy fallback: if no [listen.*] sections, use [general] listen_port
    if listen_ports.is_empty() {
        if let Some(port_str) = general.get("listen_port") {
            if let Ok(port) = port_str.parse::<u16>() {
                // Map to preset if it matches, otherwise custom
                let matching_preset = PRESET_LISTEN_PORTS.iter().find(|(_, p, _)| *p == port);
                if let Some((key, _, label)) = matching_preset {
                    listen_ports.push(ListenPortConfig {
                        key: key.to_string(), port, enabled: true,
                        label: label.to_string(), is_preset: true,
                    });
                } else {
                    listen_ports.push(ListenPortConfig {
                        key: "custom_1".to_string(), port, enabled: true,
                        label: String::new(), is_preset: false,
                    });
                }
            }
        }
    }

    // Ensure all presets exist (disabled if not in config)
    for (key, port, label) in PRESET_LISTEN_PORTS {
        if !listen_ports.iter().any(|lp| lp.key == *key) {
            listen_ports.push(ListenPortConfig {
                key: key.to_string(), port: *port, enabled: false,
                label: label.to_string(), is_preset: true,
            });
        }
    }

    // Parse targets (unchanged)
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

    Some(Config { listen_ports, targets, launch_on_startup, minimize_to_tray })
}
```

- [ ] **Step 4: Update save_config**

```rust
fn save_config(
    path: &PathBuf,
    listen_ports: &[ListenPortConfig],
    targets: &[(String, String)],
    launch_on_startup: bool,
    minimize_to_tray: bool,
) {
    let mut conf = Ini::new();
    conf.with_section(Some("general"))
        .set("launch_on_startup", launch_on_startup.to_string())
        .set("minimize_to_tray", minimize_to_tray.to_string());

    for lp in listen_ports {
        let mut section = conf.with_section(Some(format!("listen.{}", lp.key)));
        section.set("port", lp.port.to_string())
               .set("enabled", lp.enabled.to_string());
        if !lp.label.is_empty() {
            section.set("label", &lp.label);
        }
    }

    for (i, (address, note)) in targets.iter().enumerate() {
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
```

- [ ] **Step 5: Build and verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: Compilation errors related to call sites (expected — will fix in Task 3)

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: multi-port config struct, load, and save"
```

---

### Task 2: Update Slint UI with Tabbed Layout

**Files:**
- Modify: `ui/main.slint`

- [ ] **Step 1: Add ListenPort struct and update AppState**

Add struct and new properties/callbacks to AppState:

```slint
struct ListenPort {
    key: string,
    port: string,
    enabled: bool,
    label: string,
    is-preset: bool,
}
```

In `AppState`, replace `listen-port` and add tab state:

```slint
// Replace: in-out property <string> listen-port: "9301";
in-out property <[ListenPort]> listen-ports: [];
in-out property <int> active-tab: 0;  // 0 = Listen, 1 = Destinations

// Replace: callback update-listen-port(string);
callback toggle-listen-port(int, bool);
callback update-listen-port-number(int, string);
callback add-custom-listen-port();
callback remove-listen-port(int);
callback switch-tab(int);
```

- [ ] **Step 2: Create ListenPortRow component**

```slint
component ListenPortRow inherits HorizontalLayout {
    in property <int> index;
    in-out property <string> port;
    in-out property <string> label;
    in property <bool> enabled;
    in property <bool> is-preset;
    height: 36px;
    spacing: 8px;

    AppCheckBox {
        text: "";
        checked: enabled;
        toggled => { AppState.toggle-listen-port(index, self.checked); }
    }

    Text {
        text: label;
        color: enabled ? #fafafa : #52525b;
        font-size: 12px;
        vertical-alignment: center;
        min-width: 120px;
    }

    LineEdit {
        text: port;
        input-type: number;
        width: 70px;
        enabled: !is-preset;
        edited(value) => {
            AppState.update-listen-port-number(index, value);
        }
    }

    Rectangle { horizontal-stretch: 1; }

    if !is-preset : VerticalLayout {
        alignment: center;
        RedButton {
            text: "Remove";
            clicked => { AppState.remove-listen-port(index); }
        }
    }
}
```

- [ ] **Step 3: Create tab bar and replace middle content**

Replace the Listen Port section and Destinations section (lines 276-321 of current `main.slint`) with:

```slint
// Tab bar
HorizontalLayout {
    height: 32px;
    spacing: 0px;

    Rectangle {
        min-width: 80px;
        height: 32px;
        border-radius: 6px;
        background: AppState.active-tab == 0 ? #1e3a5f : transparent;

        listen-tab-ta := TouchArea {
            mouse-cursor: pointer;
            clicked => { AppState.switch-tab(0); }
        }
        Text {
            text: "Listen (" + AppState.listen-ports.length + ")";
            color: AppState.active-tab == 0 ? #60a5fa : #a1a1aa;
            font-size: 12px;
            horizontal-alignment: center;
            vertical-alignment: center;
        }
    }

    Rectangle {
        min-width: 100px;
        height: 32px;
        border-radius: 6px;
        background: AppState.active-tab == 1 ? #1e3a5f : transparent;

        dest-tab-ta := TouchArea {
            mouse-cursor: pointer;
            clicked => { AppState.switch-tab(1); }
        }
        Text {
            text: "Destinations (" + AppState.targets.length + ")";
            color: AppState.active-tab == 1 ? #60a5fa : #a1a1aa;
            font-size: 12px;
            horizontal-alignment: center;
            vertical-alignment: center;
        }
    }

    Rectangle { horizontal-stretch: 1; }
}

// Tab content
ScrollView {
    vertical-stretch: 1;

    VerticalLayout {
        spacing: 8px;

        // Listen tab
        if AppState.active-tab == 0 : VerticalLayout {
            spacing: 8px;

            for lp[index] in AppState.listen-ports : ListenPortRow {
                index: index;
                port: lp.port;
                label: lp.label;
                enabled: lp.enabled;
                is-preset: lp.is-preset;
            }

            HorizontalLayout {
                height: 36px;
                BlueButton {
                    text: "Add Custom Port";
                    clicked => { AppState.add-custom-listen-port(); }
                }
                Rectangle { horizontal-stretch: 1; }
            }
        }

        // Destinations tab
        if AppState.active-tab == 1 : VerticalLayout {
            spacing: 8px;

            HorizontalLayout {
                height: 30px;
                spacing: 8px;
                BlueButton {
                    text: "Add New Destination";
                    clicked => { AppState.add-target(); }
                }
                Rectangle { horizontal-stretch: 1; }
            }

            for target[index] in AppState.targets : TargetRow {
                index: index;
                address: target.address;
                note: target.note;
                validation-error: target.validation-error;
                is-duplicate: target.is-duplicate;
            }
        }
    }
}
```

- [ ] **Step 4: Update status text to show enabled port count**

In the status bar, update the status text to show which ports are active (e.g. "Listening on ports 9301, 20888").

- [ ] **Step 5: Commit**

```bash
git add ui/main.slint
git commit -m "feat: tabbed UI with Listen and Destinations tabs"
```

---

### Task 3: Wire Up Rust Callbacks and Multi-Port Binding

**Files:**
- Modify: `src/main.rs` (all GUI callback wiring and start/stop logic)

- [ ] **Step 1: Update SavedState for multi-port**

```rust
#[derive(Clone)]
struct SavedState {
    listen_ports: Vec<ListenPortConfig>,
    targets: Vec<(String, String)>,
}

impl SavedState {
    fn from_ui(state: &AppState) -> Self {
        let lp_model = state.get_listen_ports();
        let listen_ports = (0..lp_model.row_count())
            .map(|i| {
                let lp = lp_model.row_data(i).unwrap();
                ListenPortConfig {
                    key: lp.key.to_string(),
                    port: lp.port.to_string().parse().unwrap_or(0),
                    enabled: lp.enabled,
                    label: lp.label.to_string(),
                    is_preset: lp.is_preset,
                }
            })
            .collect();
        let model = state.get_targets();
        let targets = (0..model.row_count())
            .map(|i| {
                let t = model.row_data(i).unwrap();
                (t.address.to_string(), t.note.to_string())
            })
            .collect();
        Self { listen_ports, targets }
    }

    fn matches_ui(&self, state: &AppState) -> bool {
        let lp_model = state.get_listen_ports();
        if self.listen_ports.len() != lp_model.row_count() {
            return false;
        }
        for (i, lp) in self.listen_ports.iter().enumerate() {
            let ui_lp = lp_model.row_data(i).unwrap();
            if lp.port.to_string() != ui_lp.port.as_str()
                || lp.enabled != ui_lp.enabled
                || lp.label != ui_lp.label.as_str()
            {
                return false;
            }
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
```

- [ ] **Step 2: Wire new callbacks — switch-tab, toggle-listen-port, update-listen-port-number, add-custom-listen-port, remove-listen-port**

Wire `switch-tab`:
```rust
main_window.global::<AppState>().on_switch_tab(move |tab| {
    let w = w.upgrade().unwrap();
    w.global::<AppState>().set_active_tab(tab);
});
```

Wire `toggle-listen-port`:
```rust
// Toggle enabled state at index, mark pending changes
```

Wire `update-listen-port-number`:
```rust
// Update port number at index, mark pending changes
```

Wire `add-custom-listen-port`:
```rust
// Add new ListenPort { key: "custom_N", port: "0", enabled: true, label: "", is_preset: false }
```

Wire `remove-listen-port`:
```rust
// Remove at index (only if !is_preset), mark pending changes
```

- [ ] **Step 3: Update load config → UI mapping**

Replace the single `set_listen_port` call with mapping `Vec<ListenPortConfig>` to the Slint `ListenPort` model:

```rust
let listen_ports: Vec<ListenPort> = config
    .listen_ports
    .iter()
    .map(|lp| ListenPort {
        key: SharedString::from(&lp.key),
        port: SharedString::from(lp.port.to_string()),
        enabled: lp.enabled,
        label: SharedString::from(&lp.label),
        is_preset: lp.is_preset,
    })
    .collect();
state.set_listen_ports(ModelRc::new(VecModel::from(listen_ports)));
```

- [ ] **Step 4: Update save_config call sites**

All places that call `save_config` need to pass `listen_ports` instead of `listen_port`. Extract listen ports from UI model:

```rust
fn extract_listen_ports_from_ui(state: &AppState) -> Vec<ListenPortConfig> {
    let model = state.get_listen_ports();
    (0..model.row_count())
        .map(|i| {
            let lp = model.row_data(i).unwrap();
            ListenPortConfig {
                key: lp.key.to_string(),
                port: lp.port.to_string().parse().unwrap_or(0),
                enabled: lp.enabled,
                label: lp.label.to_string(),
                is_preset: lp.is_preset,
            }
        })
        .collect()
}
```

Update `on_save_config`, `on_toggle_launch_on_startup`, `on_toggle_minimize_to_tray` to use this.

- [ ] **Step 5: Update on_start to bind multiple ports**

Replace single socket bind with loop over enabled listen ports. Each gets its own receiver thread, all sharing the same ring + head + sender threads:

```rust
main_window.global::<AppState>().on_start(move || {
    // ... parse targets as before ...

    let lp_model = state.get_listen_ports();
    let enabled_ports: Vec<u16> = (0..lp_model.row_count())
        .filter_map(|i| {
            let lp = lp_model.row_data(i).unwrap();
            if lp.enabled {
                lp.port.to_string().parse::<u16>().ok()
            } else {
                None
            }
        })
        .collect();

    if enabled_ports.is_empty() {
        state.set_status_text(SharedString::from("Enable at least one listen port"));
        return;
    }

    // Bind all ports
    let mut sockets = Vec::new();
    for port in &enabled_ports {
        let bind_addr = format!("0.0.0.0:{}", port);
        match UdpSocket::bind(&bind_addr) {
            Ok(s) => {
                s.set_read_timeout(Some(std::time::Duration::from_millis(500))).ok();
                tune_socket(&s);
                sockets.push(s);
            }
            Err(e) => {
                state.set_status_text(SharedString::from(format!(
                    "Bind failed on port {}: {}", port, e
                )));
                return;
            }
        }
    }

    let ring = Arc::new(BroadcastRing::new(RING_CAPACITY));
    let ring_head = Arc::new(AtomicU64::new(0));
    let sender_stop = Arc::new(AtomicBool::new(false));

    spawn_ring_senders(&targets, &ring, &ring_head, &sender_stop);

    stop_flag.store(false, Ordering::Relaxed);
    packet_count.store(0, Ordering::Relaxed);
    state.set_running(true);
    state.set_packets_forwarded(0);

    let port_list: String = enabled_ports.iter()
        .map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
    state.set_status_text(SharedString::from(format!(
        "Listening on port{} {}", if enabled_ports.len() > 1 { "s" } else { "" }, port_list
    )));

    // Spawn one receiver thread per socket
    let stop = stop_flag.clone();
    let count = packet_count.clone();
    let w2 = w.clone();

    let h = thread::spawn(move || {
        let mut handles = Vec::new();
        for socket in sockets {
            let ring = ring.clone();
            let ring_head = ring_head.clone();
            let stop = stop.clone();
            let count = count.clone();
            handles.push(thread::spawn(move || {
                let mut buf = [0u8; 65535];
                loop {
                    if stop.load(Ordering::Relaxed) { break; }
                    let (len, _src) = match socket.recv_from(&mut buf) {
                        Ok(result) => result,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => continue,
                        Err(_) => continue,
                    };
                    ring.publish(&buf[..len]);
                    ring_head.fetch_add(1, Ordering::Release);
                    count.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }
        // UI update loop
        let mut last_ui_update = Instant::now();
        let mut last_pps_count: u64 = 0;
        loop {
            if stop.load(Ordering::Relaxed) {
                sender_stop.store(true, Ordering::Relaxed);
                for h in handles { let _ = h.join(); }
                break;
            }
            thread::sleep(std::time::Duration::from_millis(500));
            let n = count.load(Ordering::Relaxed);
            let elapsed = last_ui_update.elapsed();
            if elapsed.as_millis() >= 1000 {
                let pps = ((n - last_pps_count) as f64 / elapsed.as_secs_f64()) as i32;
                last_pps_count = n;
                last_ui_update = Instant::now();
                let _ = w2.upgrade_in_event_loop(move |main_window| {
                    let state = main_window.global::<AppState>();
                    state.set_packets_forwarded(n as i32);
                    state.set_packets_per_second(pps);
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
```

- [ ] **Step 6: Build and verify compilation**

Run: `cargo build`
Expected: Successful compilation

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire multi-port callbacks and multi-socket binding"
```

---

### Task 4: Update Headless Mode for Multi-Port

**Files:**
- Modify: `src/main.rs:254-314` (run_headless function)

- [ ] **Step 1: Update run_headless to bind multiple ports**

```rust
fn run_headless(config_path: PathBuf) {
    let config = load_config(&config_path).unwrap_or_else(|| {
        eprintln!("Failed to load config '{}'", config_path.display());
        std::process::exit(1);
    });

    if config.targets.is_empty() {
        eprintln!("No [forward.*] sections found in config");
        std::process::exit(1);
    }

    let enabled_ports: Vec<&ListenPortConfig> = config.listen_ports.iter()
        .filter(|lp| lp.enabled).collect();

    if enabled_ports.is_empty() {
        eprintln!("No enabled listen ports in config");
        std::process::exit(1);
    }

    let targets: Vec<SocketAddr> = config.targets.iter()
        .map(|(address, _note)| {
            use std::net::ToSocketAddrs;
            address.parse().unwrap_or_else(|_| {
                address.to_socket_addrs().ok().and_then(|mut addrs| addrs.next())
                    .unwrap_or_else(|| { eprintln!("Invalid address: {}", address); std::process::exit(1); })
            })
        })
        .collect();

    let ring = Arc::new(BroadcastRing::new(RING_CAPACITY));
    let head = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    spawn_ring_senders(&targets, &ring, &head, &stop);

    for lp in &enabled_ports {
        println!("Listening on UDP port {} ({})", lp.port, lp.label);
    }
    for target in &targets {
        println!("  Forwarding to {}", target);
    }

    let mut handles = Vec::new();
    for lp in &enabled_ports {
        let bind_addr = format!("0.0.0.0:{}", lp.port);
        let socket = UdpSocket::bind(&bind_addr).unwrap_or_else(|e| {
            eprintln!("Failed to bind to {}: {}", bind_addr, e);
            std::process::exit(1);
        });
        tune_socket(&socket);

        let ring = ring.clone();
        let head = head.clone();
        handles.push(thread::spawn(move || {
            let mut buf = [0u8; 65535];
            loop {
                let (len, _src) = match socket.recv_from(&mut buf) {
                    Ok(result) => result,
                    Err(e) => { eprintln!("recv error: {}", e); continue; }
                };
                ring.publish(&buf[..len]);
                head.fetch_add(1, Ordering::Release);
            }
        }));
    }

    // Block on all receiver threads
    for h in handles { let _ = h.join(); }
}
```

- [ ] **Step 2: Build and test headless mode**

Run: `cargo build && echo "OK"`
Expected: OK

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: headless mode supports multiple listen ports"
```

---

### Task 5: Update Config Example and Default State

**Files:**
- Modify: `config.ini.example`

- [ ] **Step 1: Update config.ini.example**

```ini
[general]
launch_on_startup = false
minimize_to_tray = false

[listen.raceiq]
port = 9301
enabled = true
label = RaceIQ

[listen.f1]
port = 20888
enabled = true
label = F1 24

[listen.forza]
port = 4843
enabled = false
label = Forza Motorsport

[forward.1]
ip = 127.0.0.1
port = 5301
note = RaceIQ
```

- [ ] **Step 2: Update default UI state when no config exists**

In `main()`, when `load_config` returns None, populate with presets (RaceIQ enabled, others disabled):

```rust
if config.is_none() {
    let defaults: Vec<ListenPort> = PRESET_LISTEN_PORTS.iter()
        .map(|(key, port, label)| ListenPort {
            key: SharedString::from(*key),
            port: SharedString::from(port.to_string()),
            enabled: *key == "raceiq",
            label: SharedString::from(*label),
            is_preset: true,
        })
        .collect();
    state.set_listen_ports(ModelRc::new(VecModel::from(defaults)));
}
```

- [ ] **Step 3: Commit**

```bash
git add config.ini.example src/main.rs
git commit -m "feat: update config example and default preset state"
```

---

### Task 6: Remove Old listen_port References and Clean Up

**Files:**
- Modify: `src/main.rs`
- Modify: `ui/main.slint`

- [ ] **Step 1: Remove old `update-listen-port` callback and single port UI references**

Ensure the old `listen-port` property and `update-listen-port(string)` callback are fully removed from AppState and all Rust callback wiring.

- [ ] **Step 2: Full build and manual smoke test**

Run: `cargo build --release`
Launch the app, verify:
- Two tabs visible (Listen | Destinations)
- Listen tab shows 3 presets (RaceIQ enabled, F1 enabled, Forza disabled)
- Can toggle presets on/off
- Can add custom port
- Can remove custom port (not presets)
- Destinations tab shows existing targets
- Save & Apply restarts with all enabled ports
- Status bar shows "Listening on ports 9301, 20888"

- [ ] **Step 3: Commit**

```bash
git add src/main.rs ui/main.slint
git commit -m "feat: clean up old single-port references"
```

---

### Task 7: Update Benchmark for Multi-Port

**Files:**
- Modify: `benches/forwarding_bench.rs`

- [ ] **Step 1: Update write_config to use new format**

```rust
fn write_config(
    path: &std::path::Path,
    listen_port: u16,
    num_targets: usize,
    target_port_base: u16,
) {
    let mut f = std::fs::File::create(path).expect("Failed to create config");
    writeln!(f, "[general]").unwrap();
    writeln!(f, "\n[listen.bench]").unwrap();
    writeln!(f, "port = {}", listen_port).unwrap();
    writeln!(f, "enabled = true").unwrap();
    writeln!(f, "label = Benchmark").unwrap();
    for i in 0..num_targets {
        writeln!(f, "\n[forward.{}]", i + 1).unwrap();
        writeln!(f, "ip = 127.0.0.1").unwrap();
        writeln!(f, "port = {}", target_port_base + i as u16).unwrap();
    }
}
```

- [ ] **Step 2: Build and run benchmark**

Run: `cargo build --release && cargo run --release --bin forwarding_bench`
Expected: Benchmark runs and prints results table

- [ ] **Step 3: Commit**

```bash
git add benches/forwarding_bench.rs
git commit -m "feat: update benchmark config for multi-port format"
```
