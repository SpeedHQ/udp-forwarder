# System Tray Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add system tray support so the app can run in the background with live status, click-to-open, and optional minimize-to-tray on close.

**Architecture:** The `tray-icon` crate provides cross-platform system tray. A programmatic 32x32 arrow icon is generated in `build.rs` using the `image` crate and embedded via `include_bytes!`. A `slint::Timer` polls tray events every 100ms. The `on_close_requested` callback hides the window or quits based on user preference.

**Tech Stack:** Rust, Slint 1.9, tray-icon, image crate

**Spec:** `docs/superpowers/specs/2026-03-25-system-tray-design.md`

---

### Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add tray-icon and image to Cargo.toml**

In `[dependencies]`, add:
```toml
tray-icon = "0.19"
image = { version = "0.25", default-features = false, features = ["png"] }
```

In `[build-dependencies]`, add:
```toml
image = { version = "0.25", default-features = false, features = ["png"] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add tray-icon and image dependencies for system tray"
```

---

### Task 2: Generate tray icon in build.rs

**Files:**
- Modify: `build.rs`

The icon is a thick arrow pointing top-right on a transparent 32x32 canvas, white color. Generated at build time, saved to `OUT_DIR` as PNG.

- [ ] **Step 1: Update build.rs to generate the icon**

Replace `build.rs` with:

```rust
use std::env;
use std::path::Path;

fn main() {
    slint_build::compile("ui/main.slint").unwrap();

    // Generate tray icon: 32x32 thick arrow pointing top-right
    let out_dir = env::var("OUT_DIR").unwrap();
    let icon_path = Path::new(&out_dir).join("tray_icon.png");

    let mut img = image::RgbaImage::new(32, 32);

    // Draw a thick arrow pointing top-right
    // Arrow body: diagonal line from bottom-left to top-right (thick)
    for i in 0..22 {
        for t in -2i32..=2 {
            let x = (6 + i + t).clamp(0, 31) as u32;
            let y = (25 - i + t).clamp(0, 31) as u32;
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            // Extra thickness
            let x2 = (6 + i + t).clamp(0, 31) as u32;
            let y2 = (25 - i).clamp(0, 31) as u32;
            img.put_pixel(x2, y2, image::Rgba([255, 255, 255, 255]));
        }
    }
    // Arrowhead: horizontal bar at top-right
    for x in 18..28u32 {
        for t in 0..3u32 {
            let y = (4 + t).min(31);
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    // Arrowhead: vertical bar at top-right
    for y in 4..14u32 {
        for t in 0..3u32 {
            let x = (25 + t).min(31);
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }

    img.save(&icon_path).expect("Failed to save tray icon");
    println!("cargo::rerun-if-changed=build.rs");
}
```

- [ ] **Step 2: Verify it compiles and generates the icon**

Run: `cargo build 2>&1 | head -5`
Then check icon exists: `ls $(cargo metadata --format-version 1 | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")/debug/build/udp-forwarder-*/out/tray_icon.png`

- [ ] **Step 3: Commit**

```bash
git add build.rs
git commit -m "feat: generate tray icon arrow in build.rs"
```

---

### Task 3: Add minimize_to_tray to config and Slint UI

**Files:**
- Modify: `src/main.rs` (Config struct, load_config, save_config)
- Modify: `ui/main.slint` (AppState, MainWindow)

- [ ] **Step 1: Add minimize_to_tray to AppState in main.slint**

In `ui/main.slint`, add to `AppState` global after `launch-on-startup`:

```
in-out property <bool> minimize-to-tray: false;
callback toggle-minimize-to-tray(bool);
```

- [ ] **Step 2: Add toggle checkbox in MainWindow**

In `ui/main.slint`, in the header `HorizontalLayout` (the one with "Launch on startup"), add a second `CheckBox` before the existing one:

```slint
CheckBox {
    text: "Minimize to tray";
    checked: AppState.minimize-to-tray;
    toggled => { AppState.toggle-minimize-to-tray(self.checked); }
}
```

- [ ] **Step 3: Add minimize_to_tray to Config struct in main.rs**

In `src/main.rs`, update the `Config` struct:

```rust
struct Config {
    listen_port: u16,
    targets: Vec<(String, u16, String)>,
    launch_on_startup: bool,
    minimize_to_tray: bool,
}
```

- [ ] **Step 4: Update load_config to read minimize_to_tray**

In `load_config()`, after the `launch_on_startup` line, add:

```rust
let minimize_to_tray = general.get("minimize_to_tray").map_or(false, |v| v == "true");
```

And update the return:

```rust
Some(Config { listen_port, targets, launch_on_startup, minimize_to_tray })
```

- [ ] **Step 5: Update save_config to write minimize_to_tray**

Change `save_config` signature to:

```rust
fn save_config(path: &PathBuf, listen_port: &str, targets: &[(String, String, String)], launch_on_startup: bool, minimize_to_tray: bool) {
```

In the function body, add to the `general` section:

```rust
conf.with_section(Some("general"))
    .set("listen_port", listen_port)
    .set("launch_on_startup", launch_on_startup.to_string())
    .set("minimize_to_tray", minimize_to_tray.to_string());
```

- [ ] **Step 6: Update all save_config call sites**

There are two callers of `save_config` in `main()`:

1. In `on_toggle_launch_on_startup` callback — add `state.get_minimize_to_tray()` as the last argument
2. In `on_save_config` callback — add `state.get_minimize_to_tray()` as the last argument

- [ ] **Step 7: Load minimize_to_tray into UI state**

In the config loading block in `main()` (the `if let Some(config) = load_config(...)` block), add:

```rust
state.set_minimize_to_tray(config.minimize_to_tray);
```

- [ ] **Step 8: Wire toggle_minimize_to_tray callback**

Add a new callback handler block in `main()`, similar to `on_toggle_launch_on_startup`:

```rust
{
    let w = main_window.as_weak();
    let path = config_file.clone();
    main_window.global::<AppState>().on_toggle_minimize_to_tray(move |enabled| {
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
        save_config(&path, &listen_port, &targets, state.get_launch_on_startup(), enabled);
    });
}
```

- [ ] **Step 9: Verify it compiles**

Run: `cargo check`

- [ ] **Step 10: Commit**

```bash
git add src/main.rs ui/main.slint
git commit -m "feat: add minimize-to-tray config setting and UI toggle"
```

---

### Task 4: Create system tray with menu and event handling

**Files:**
- Modify: `src/main.rs`

This is the core task. Add tray icon creation, context menu (status label + quit), click-to-show, close-to-tray, and live status updates.

- [ ] **Step 1: Add tray-icon imports**

At the top of `src/main.rs`, add:

```rust
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent, PredefinedMenuItem}};
use tray_icon::TrayIconEvent;
```

- [ ] **Step 2: Create the tray icon and menu in main(), before main_window.run()**

After the auto-start block and before `main_window.run()`, add:

```rust
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
```

- [ ] **Step 3: Add slint::Timer to poll tray events**

After creating the tray, add a timer that polls for menu and tray click events:

```rust
// Poll tray events via Slint timer
{
    let w = main_window.as_weak();
    let quit_id = quit_item_id.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
        // Handle menu events (Quit)
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == quit_id {
                slint::quit_event_loop().ok();
            }
        }
        // Handle tray icon click (show window)
        if let Ok(TrayIconEvent::Click { .. }) = TrayIconEvent::receiver().try_recv() {
            if let Some(w) = w.upgrade() {
                w.window().show().ok();
            }
        }
    });
    // Leak the timer so it lives for the duration of the app
    std::mem::forget(timer);
}
```

- [ ] **Step 4: Add on_close_requested handler**

After the tray event timer, add the close handler:

```rust
// Handle window close — hide to tray or quit
{
    let w = main_window.as_weak();
    main_window.window().on_close_requested(move || {
        let w = w.upgrade().unwrap();
        if w.global::<AppState>().get_minimize_to_tray() {
            w.window().hide().ok();
            slint::CloseRequestResponse::KeepWindowShown
        } else {
            slint::quit_event_loop().ok();
            slint::CloseRequestResponse::HideWindow
        }
    });
}
```

- [ ] **Step 5: Update the packet counter timer to also update tray status**

In the forwarding thread's `upgrade_in_event_loop` closure (the one that runs every ~1 second), add a call to update the tray menu status. Since `MenuItem` is not `Send`, we need to update it from the main thread. Store the `status_item` in an `Rc` and update it in the event loop.

Change the `status_item` creation to use `Rc`:

```rust
use std::rc::Rc;
let status_item = Rc::new(MenuItem::new("Stopped", false, None));
```

Then pass a clone into the `upgrade_in_event_loop` closure. Since the forwarding thread can't hold `Rc`, instead update the status item from a second `slint::Timer` that reads the current state:

```rust
// Timer to update tray status text from UI state
{
    let w = main_window.as_weak();
    let status = status_item.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), move || {
        if let Some(w) = w.upgrade() {
            let state = w.global::<AppState>();
            let text = if state.get_running() {
                format!("Running — {} pkt/s", state.get_packets_per_second())
            } else {
                "Stopped".to_string()
            };
            status.set_text(&text);
        }
    });
    std::mem::forget(timer);
}
```

- [ ] **Step 6: Replace main_window.run() with run_event_loop()**

At the end of `main()`, replace:
```rust
main_window.run().unwrap();
```
with:
```rust
main_window.show().unwrap();
slint::run_event_loop_until_quit().unwrap();
```

This prevents Slint from exiting when the window is hidden (minimize-to-tray). The event loop now only exits when `slint::quit_event_loop()` is called explicitly (from the Quit menu item or the close handler when minimize-to-tray is off).

- [ ] **Step 7: Verify it compiles**

Run: `cargo check`

- [ ] **Step 8: Manual test**

Run: `cargo run`

Verify:
1. Tray icon appears in system tray (arrow icon)
2. Right-click shows menu with status ("Stopped" or "Running — X pkt/s") and "Quit"
3. Clicking tray icon shows/brings window to front
4. With "Minimize to tray" checked, closing the window hides it (tray icon remains)
5. With "Minimize to tray" unchecked, closing the window quits the app
6. "Quit" from tray menu fully exits the app
7. Status text in tray menu updates when forwarding is active

- [ ] **Step 9: Commit**

```bash
git add src/main.rs
git commit -m "feat: add system tray with click-to-show, status, and close-to-tray"
```

---

### Task 5: Update CI for Linux tray dependency

**Files:**
- Modify: `.github/workflows/build.yml`

- [ ] **Step 1: Add libayatana-appindicator3-dev to Linux build**

In `.github/workflows/build.yml`, add a step before the Build step, conditional on Linux:

```yaml
- name: Install Linux dependencies
  if: contains(matrix.target, 'linux')
  run: sudo apt-get update && sudo apt-get install -y libayatana-appindicator3-dev
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/build.yml
git commit -m "ci: add libayatana-appindicator3-dev for Linux tray support"
```

---

### Task 6: Update config example

**Files:**
- Modify: `config.ini.example`

- [ ] **Step 1: Add minimize_to_tray to config.ini.example**

Add `minimize_to_tray = true` to the `[general]` section.

- [ ] **Step 2: Commit**

```bash
git add config.ini.example
git commit -m "docs: add minimize_to_tray to config example"
```
