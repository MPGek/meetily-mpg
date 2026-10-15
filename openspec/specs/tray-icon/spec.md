# tray-icon Specification

## Purpose

Defines how the Meetily system tray icon responds to mouse clicks: a left click restores the main window and a right click opens the tray menu.

## Requirements

### Requirement: Left click restores the main window
On platforms that report tray click events (Windows, macOS), a left click on the tray icon SHALL make the main window visible, un-minimized and focused, and SHALL NOT open the tray menu.

#### Scenario: Window is minimized
- **WHEN** the main window is minimized and the user left-clicks the tray icon
- **THEN** the main window is restored, shown in front and focused
- **AND** the tray menu does not open

#### Scenario: Window was closed to tray
- **WHEN** the user closed the main window (hiding it to the tray) and then left-clicks the tray icon
- **THEN** the main window becomes visible and focused with its previous state preserved

#### Scenario: Window is already visible
- **WHEN** the main window is already visible and the user left-clicks the tray icon
- **THEN** the main window is brought to the front and focused
- **AND** the window is not hidden

#### Scenario: Single action per click
- **WHEN** the user performs one left click (press and release) on the tray icon
- **THEN** the window restore action runs once, on release

### Requirement: Right click opens the tray menu
A right click on the tray icon SHALL open the tray menu with the same items and actions as before this change.

#### Scenario: Right click shows menu
- **WHEN** the user right-clicks the tray icon
- **THEN** the tray menu opens with recording controls, "Open Main Window", "Settings", "Check for Updates" and "Quit"
- **AND** the main window state is not changed by the click itself
