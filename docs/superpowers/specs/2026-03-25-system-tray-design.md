# System Tray Support

## Overview

Add system tray integration to the UDP Forwarder GUI app so it can run unobtrusively in the background, showing live status and providing quick access.

## Tray Icon

- Programmatic 32x32 PNG: thick arrow pointing top-right, white on transparent background
- Embedded as const bytes in the binary — no external asset file
- Generated using the `image` crate at build time via `build.rs`, written to `OUT_DIR`

## Tray Behavior

- **Click** — show/bring GUI window to front (note: on macOS, left-click on menu bar items conventionally shows the menu; we use `with_menu_on_left_click(false)` to get click-to-show behavior, but fall back gracefully if the platform doesn't support distinct click events)
- **Right click** — context menu:
  - Status label (disabled/non-clickable): e.g., "Running — 1,234 pkt/s" or "Stopped"
  - Separator
  - "Quit" — fully exits the app
- Status label updated via `MenuItem::set_text()` — the `MenuItem` handle is kept alive in an `Arc` accessible from the update timer

## Close-to-Tray Setting

- New `minimize_to_tray` boolean in `config.ini` under `[general]`
- New toggle in the Slint GUI alongside existing "Launch on startup"
- When enabled: closing the window hides it to tray (app keeps running)
- When disabled: closing the window quits the app (current behavior)
- Takes effect immediately when toggled (next close uses the new setting)
- Slint's `on_close_requested` callback checks this setting and returns `CloseRequestResponse::HideWindow` or `CloseRequestResponse::KeepWindowShown` + triggers quit

## Window Hide/Show

- Use `main_window.window().hide()` and `main_window.window().show()` to toggle visibility
- **Critical:** Slint's `run()` exits when all windows are hidden. To prevent this, use `slint::run_event_loop_until_quit()` instead of `main_window.run()`, or set `quit_on_last_window_closed(false)` if available. The tray icon click handler calls `main_window.window().show()` to restore.

## Event Loop Integration

- Slint owns the event loop via `main_window.run()` — we cannot insert a custom poll loop
- Use a `slint::Timer` (repeated, ~100ms interval) to call `MenuEvent::receiver().try_recv()` and `TrayIconEvent::receiver().try_recv()` to process tray events
- This is lightweight and integrates cleanly with Slint's event loop

## Config Changes

```ini
[general]
listen_port = 5300
launch_on_startup = false
minimize_to_tray = true
```

## Dependencies

- `tray-icon` — cross-platform system tray icon and menu
- `image` — programmatic icon generation (PNG encoding)

## Integration Points

- `TrayIcon` handle stored in a variable that outlives `main_window.run()` to keep the icon alive
- Tray created on main thread before the event loop starts
- Status text updated from the same 1-second packet counter logic (via `upgrade_in_event_loop`), calling `MenuItem::set_text()` on the status item
- `on_close_requested` on MainWindow checks `minimize_to_tray` setting

## Structural Changes

- `Config` struct: add `minimize_to_tray: bool` field
- `load_config()`: read `minimize_to_tray` from INI
- `save_config()`: write `minimize_to_tray` to INI
- All existing callers of `save_config` updated to pass the new field

## UI Changes (Slint)

- Add `minimize_to_tray` boolean to `AppState` global
- Add `toggle_minimize_to_tray` callback to `AppState`
- Add toggle row in the settings area of `MainWindow`

## Files Modified

- `Cargo.toml` — add `tray-icon` and `image` dependencies
- `build.rs` — generate tray icon PNG at build time
- `src/main.rs` — tray setup, close handler, icon generation, menu updates, config changes
- `ui/main.slint` — new toggle for minimize-to-tray setting

## Platform Notes

- **macOS:** tray operations must run on main thread (already the case with Slint's event loop). Left-click behavior may show menu instead of firing click event on some versions.
- **Windows:** `tray-icon` handles the notification area natively. No extra dependencies.
- **Linux:** requires `libayatana-appindicator3` (or `libappindicator3`) system package. CI workflow (`.github/workflows/build.yml`) must add `apt-get install libayatana-appindicator3-dev` to the Linux build step. Left-click may show menu on some desktop environments (AppIndicator limitation).

## Tray Icon Lifetime

The `TrayIcon` object must NOT be dropped while the app is running — dropping it removes the icon from the system tray. Store it in a `let _tray = ...` binding in `main()` that lives until the event loop exits.
