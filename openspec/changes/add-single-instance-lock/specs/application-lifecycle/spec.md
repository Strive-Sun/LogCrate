## ADDED Requirements

### Requirement: Single active application instance

The desktop application MUST acquire a platform-backed per-user single-instance lock before creating its main window, tray, watchers, or background workers. If acquisition fails because another LogCrate process owns the lock, the new process MUST exit cleanly without creating those startup side effects. The first process MUST remain unaffected and continue its normal lifecycle; an updater-requested restart MUST be allowed after the old process releases the lock.

#### Scenario: First launch acquires the lock

- **WHEN** no LogCrate process for the current user owns the instance lock
- **THEN** the process acquires the lock and continues normal startup, including window and tray creation

#### Scenario: Duplicate launch is rejected

- **WHEN** a second LogCrate process starts while the first process owns the instance lock
- **THEN** the second process exits cleanly without creating a window, tray icon, watcher, or background task

#### Scenario: Crash or updater restart releases ownership

- **WHEN** the owning process exits normally, crashes, or hands off to an updater restart
- **THEN** the operating system releases the lock and the replacement process can acquire it
