# Tasks

## 1. Tray click handling

- [x] 1.1 In `create_tray` (`frontend/src-tauri/src/tray.rs`), add `.show_menu_on_left_click(false)` to the `TrayIconBuilder` so the menu opens only on right click; verify `cargo check` in `frontend/src-tauri` passes
- [x] 1.2 Add `.on_tray_icon_event(...)` that calls the existing `focus_main_window` only for `TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }` (Windows sends a `Click` on both press and release); verify `cargo check` passes with no new warnings

## 2. Manual verification (Windows)

- [ ] 2.1 Run the app, minimize the main window, left-click the tray icon; verify the window is restored and focused and no menu appears
- [ ] 2.2 Close the main window (hides to tray), left-click the tray icon; verify the window reappears on the same page
- [ ] 2.3 With the window already visible, left-click the tray icon; verify it comes to front and is not hidden
- [ ] 2.4 Right-click the tray icon; verify the menu opens with the same items and "Open Main Window" / "Start Recording" still work

## Workflow follow-up

- Run `graphify update .` after the code change.
- macOS check is not part of this change (no macOS machine); Linux keeps menu-on-any-click by platform limitation.
