# Review decisions: rivets-u2mz

| finding-id | finding | reviewer | evidence-state | evidence | decision | fix | note |
|---|---|---|---|---|---|---|---|
| F1 | Tools::info releases its storage-resolution guard before fetching cached information; concurrent explicit roots can evict that entry. Return the report under the same context guard. | ReportingReview | Verified | Original `information_survives_concurrent_cache_eviction` failed with WorkspaceNotInitialized under 64 concurrent valid roots; paired-guard fix passed. | Accept | Slice 1: shared-lock report fast path; on miss, initialize and clone information under one write guard. | Existing C2 snapshot contract preserved. Original RED/restored GREEN, all four reporting parity tests and all eight Slice 1 gates PASS; final 1,227-test workspace integration PASS. |
