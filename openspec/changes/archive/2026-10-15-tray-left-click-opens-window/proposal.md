# Proposal

## Why

Both left and right clicks on the tray icon currently open the context menu, so bringing back a minimized or closed (hidden) Meetily window takes two clicks: icon, then "Open Main Window". The usual desktop convention is that a left click restores the app and a right click shows the menu.

## What Changes

- Left click on the tray icon restores the main window: unminimizes it if minimized, shows it if hidden (closing the window only hides it), and focuses it.
- Left click no longer opens the tray menu.
- Right click keeps opening the existing tray menu; its contents and actions are unchanged.
- If the window is already visible, a left click only brings it to the front. It does not toggle or hide the window.

## Capabilities

### New Capabilities
- `tray-icon`: how the system tray icon responds to user clicks (window restore on left click, context menu on right click).

### Modified Capabilities
<!-- none: no existing spec covers the tray -->

## Impact

- Code: `frontend/src-tauri/src/tray.rs` (`create_tray` only). Reuses the existing `focus_main_window`.
- No new dependencies. Tauri 2.6.2 with the `tray-icon` feature already provides the needed builder options and click events.
- Platform note: on Linux (libappindicator) the tray emits no click events and always shows the menu, so the new left-click behavior applies to Windows and macOS only.
