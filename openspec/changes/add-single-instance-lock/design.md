## Context

The guard must run before desktop window creation and before any startup worker is scheduled. It must work on Windows and macOS, avoid stale lock files after crashes, and not interfere with an intentional updater restart.

## Goals / Non-Goals

- Goals: one process per user session; deterministic second-launch behavior; no duplicate side effects; preserve normal and updater exits.
- Non-Goals: cross-user machine-wide exclusivity, forwarding arbitrary command-line files, or changing tray/window behavior of the first process.

## Decisions

- Use a platform-backed named single-instance primitive through a maintained Tauri single-instance plugin rather than an ad-hoc lock file. The operating system releases ownership when the process exits or crashes.
- Register the plugin before `setup`; the second-instance callback only records/focuses the already-created main window and never constructs another application state.
- Treat failure to acquire the primitive as a normal early exit with a concise diagnostic; the first instance continues startup unchanged.

## Risks / Trade-offs

- Plugin behavior and callback APIs become a build dependency; pin the major version compatible with Tauri 2 and cover callback registration in compile-time tests.
- A second launch cannot be used to open a new path in this change; forwarding arguments remains a separate capability.

## Migration Plan

No data migration. Existing installations acquire the named primitive on their next launch; uninstall and updater restart remain ordinary process lifecycle operations.

## Open Questions

- None for this change.
