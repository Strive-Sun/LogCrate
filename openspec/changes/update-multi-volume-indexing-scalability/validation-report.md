# Validation Report

## 4.1 Scheduler and retained-resource matrix

The deterministic Windows scheduler fixture ran logical scope counts 1, 4, 5, 10, and 64. The counters represent resources retained by the scheduling model; the stage retention fixture separately verifies that completed stages leave no SQLite/WAL/SHM files.

| Scopes | Runnable | Workers | Requests | Pipes | Stages | Batches | Records | Queued | Retries | Threads | Stage handles | Retained stage files | Retained RSS/disk model |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 1 | 1 | 1 | 1 | 1 | 257 | 0 | 0 | 1 | 1 | 0 | 0 |
| 4 | 4 | 4 | 4 | 4 | 4 | 4 | 1028 | 0 | 0 | 4 | 4 | 0 | 0 |
| 5 | 5 | 4 | 4 | 4 | 4 | 4 | 1028 | 0 | 0 | 4 | 4 | 0 | 0 |
| 10 | 8 | 4 | 4 | 4 | 4 | 4 | 1028 | 2 | 0 | 4 | 4 | 0 | 0 |
| 64 | 8 | 4 | 4 | 4 | 4 | 4 | 1028 | 56 | 0 | 4 | 4 | 0 | 0 |

Evidence:

- `scheduler_resource_matrix_is_constant_at_w_and_four_w`
- `completed_stage_retention_is_constant_from_one_to_sixty_four_scopes`
- `volume_scheduler_keeps_large_scope_sets_bounded`

At `N=W`, active resources peak at four. At `N=4W`, the runnable window is eight while the same active-resource peaks remain four. Scope identity/status metadata and the queued count are the only values that grow with N.

## 4.2 Failure injection and recovery matrix

| Injection | Expected path | Evidence |
|---|---|---|
| IPC busy/missing | bounded client retry, then persisted `waitingToRetry` | `client_retry_backoff_is_bounded_and_categorized`; `recovery_matrix_survives_process_reopen_and_clears_nonexternal_failures` |
| Service exit/stopped | one automatic start per round; persisted next-round recovery | `client_recovery_round_claims_at_most_one_automatic_service_start`; recovery matrix |
| Protocol/install failure | readable volume falls back to `folderScan`, never blocked | `readable_root_falls_back_from_service_failure_to_folder_provider`; recovery matrix |
| Temporary offline then accessible | blocked evidence is woken by lightweight accessibility probe and reclaimed | `blocked_probe_only_checks_accessibility_before_waking_the_queue`; recovery matrix |
| Process reopen | all per-volume obligations reload from SQLite and complete independently | recovery matrix |

The combined process-reopen fixture persists four independent failures, reconstructs the manager, wakes the temporary-offline scope, claims all four with the W=4 budget, and clears only after successful completion. No application restart API is called; recovery is an indexing operation within the current process or the next natural application start.

## 4.3 External unsatisfiable matrix

The external-block fixture covers the only accepted terminal categories: media/device damage, stable volume offline/removal, and target-volume access denied. Three blocked scopes are persisted beside one completed scope. No blocked row is claimable by the active recovery scheduler, the completed scope remains available, and the operation resolves to `attentionRequired` rather than `scanning`, `ready`, or `converged`.

Evidence:

- `external_block_matrix_enters_attention_after_other_scopes_complete`
- `blocked_classification_requires_explicit_volume_evidence`
- `blocked_probe_only_checks_accessibility_before_waking_the_queue`
- `operation_gate_distinguishes_recovery_attention_and_scope_changes`

## 4.4 Windows IPC capacity and recovery acceptance

On Windows at `2026-08-17T13:12:25.7650270+08:00`, the real named-pipe fixture held all four business requests open, admitted an extra client far enough to return the v2 `429` busy envelope, released slots after both partial-frame and ordinary disconnects, accepted replacement clients, and stopped through the bounded wake path. The transport namespace remained available while business capacity was full, and no pipe instance remained after the requested stop.

Evidence:

- `real_pipe_saturation_reconnect_disconnect_and_stop_are_bounded`: 1 passed
- `pipe_accept_capacity_is_separate_from_business_capacity`: 1 passed
- `concurrent_client_storm_is_bounded`: 1 passed
- Windows System log, provider `Service Control Manager`, acceptance window beginning at the timestamp above: 0 LogCrate service events and 0 abnormal-stop events (`7031`/`7034`)
- Installed `LogCrateIndex` service after acceptance: `Running`, manual start

## 4.5 Stable-volume identity and drive-letter reuse matrix

The stable-identity fixture now exercises both sides of drive-letter churn in one database. Volume A begins on `D:`, owns a generation-41 recovery obligation, an isolated MFT stage, USN journal 7/next-USN 9, and a completed snapshot. After the same Volume GUID remounts on `E:`, the same scope and recovery row move to `E:` while preserving generation, attempt history, USN position, and completion state.

A different Volume GUID then occupies `D:` with its own serial, USN journal 17/next-USN 19, incomplete snapshot state, and separately tagged stage file. It does not inherit Volume A's recovery row, stage metadata, USN cursor, or completed flag. The database retains exactly two identities despite the mount changes.

Evidence:

- `stable_volume_state_survives_mount_changes_and_isolates_letter_reuse`
- `volume_guid_paths_are_normalized_without_drive_letters`
- MFT `stage_metadata.volume` and recovery/scoped-state assertions in the combined fixture

## 4.6 Performance and final L3 gates

The complete measurements, raw sample table, medians, environment, baseline comparison, directory-change recovery, and real WebView input evidence are recorded in `benchmark.md`.

Performance acceptance:

- Three real Release C:/D: application rebuilds: median scheduling 175 ms, D/C MFT enumeration 12.757/16.138 s, first searchable result 175 ms, all-volume query ready 47.824 s, ready publication delay 54 ms, and converged 166.793 s. Against the archived three-run implementation baseline, first searchable improved 98.7% and query ready improved 14.8%.
- Three real D: directory-change rebuilds: median enumeration 7.084 s and query ready 23.406 s; all proof-file queries matched.
- Isolated Release Tauri/WebView2 while real C:/D: indexing remained `scanning`: 100 production input-to-next-frame samples, p95 17.5 ms. The isolated process, CDP listener, and acceptance data roots were removed afterward; the production application profile was not touched.
- All samples retained per-scope `ready`, final operation `converged`, matching searchable counts, and representative C:/D: query results.

Final L3 evidence on the final implementation diff:

| Scope | Command or step | Result | Notes |
|---|---|---|---|
| Rust full test | `cargo test --manifest-path src-tauri/Cargo.toml` | 281 passed, 0 failed, 9 ignored | The ignored set contains the explicitly invoked real-device/performance entries and existing environment probes. |
| Rust format/static/build | `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check`; all-target Clippy with `-D warnings`; all-target check; index-service feature check | all passed | No warnings from Rust gates. |
| Frontend | `npm test`; `npx tsc --noEmit`; `npm run lint`; `npm run format:check`; `npm run build` | 144 passed, 0 failed; all static/format/build gates passed | Existing React `act(...)` warnings remain test-runner noise with no failures and no 4.6 frontend changes. |
| OpenSpec/diff | `openspec validate update-multi-volume-indexing-scalability --strict`; `git diff --check` | passed | Re-run after final report/task/state updates. |

Task 4.6 introduced no application-close, application-restart, or user-restart path. Query publication, SQLite/USN materialization, and recovery remain background work in the current process or the next natural application start.
