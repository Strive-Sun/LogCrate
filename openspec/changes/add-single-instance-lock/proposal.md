# Change: Prevent duplicate LogCrate processes

## Why

Launching LogCrate more than once currently creates competing desktop processes that can duplicate watchers, background indexing, tray state, and resource usage. Users need a single active application process per user session.

## What Changes

- Add a cross-platform single-instance guard to the desktop application startup path.
- Make a second launch exit without creating a window, tray icon, watcher, or background task.
- Keep updater-driven restart and normal process exit compatible with the guard.
- Add deterministic Rust coverage for first-instance acquisition and second-instance rejection.

## Impact

- Affected spec: `application-lifecycle`.
- Affected code: `src-tauri/src/lib.rs`, Cargo dependencies, and lifecycle tests.
- No frontend IPC or persisted user data changes.
