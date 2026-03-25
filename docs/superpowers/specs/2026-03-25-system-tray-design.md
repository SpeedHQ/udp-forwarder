# System Tray Support

## Overview

Add system tray integration to the UDP Forwarder GUI app so it can run unobtrusively in the background, showing live status and providing quick access.

## Tray Icon

- Programmatic 32x32 PNG: thick arrow pointing top-right, white on transparent background
- Encoded at build time or embedded as const bytes — no external asset file
- Generated using the `image` crate

## Tray Behavior

- **Left click** — show/bring GUI window to front
- **Right click** — context menu:
  - Status label (disabled/non-clickable): e.g., "Running — 1,234 pkt/s" or "Stopped"
  - Separator
  - "Quit" — fully exits the app

## Close-to-Tray Setting

- New `minimize_to_tray` boolean in `config.ini` under `[general]`
- New toggle in the Slint GUI alongside existing "Launch on startup"
- When enabled: closing the window hides it to tray (app keeps running)
- When disabled: closing the window quits the app (current behavior)
- Slint's `on_close_requested` callback checks this setting

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

- Tray created on main thread before `main_window.run()`
- Status text in tray menu updated on the same 1-second timer that already updates the GUI pkt/s counter (via `upgrade_in_event_loop`)
- `on_close_requested` on MainWindow checks `minimize_to_tray` and either hides window or allows quit
- `Window::set_visible(false/true)` used to hide/show the Slint window

## UI Changes (Slint)

- Add `minimize_to_tray` boolean to `AppState` global
- Add `toggle_minimize_to_tray` callback to `AppState`
- Add toggle row in the settings area of `MainWindow`

## Files Modified

- `Cargo.toml` — add `tray-icon` and `image` dependencies
- `src/main.rs` — tray setup, close handler, icon generation, menu updates
- `ui/main.slint` — new toggle for minimize-to-tray setting

## Platform Notes

- macOS: tray operations must run on main thread (already the case with Slint's event loop)
- Windows: `tray-icon` handles the notification area natively
- Linux: uses libappindicator or StatusNotifierItem depending on desktop environment
