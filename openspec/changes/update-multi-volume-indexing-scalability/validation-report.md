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
