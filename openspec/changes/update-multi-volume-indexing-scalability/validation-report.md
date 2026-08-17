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
