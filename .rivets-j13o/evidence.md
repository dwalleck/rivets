# Evidence: rivets-j13o

## Premise checklist

| ID | Candidate premise | Smallest question | Verdict |
|----|-------------------|-------------------|---------|
| P1 | A nonblocking exclusive standard-library file lock distinguishes contention from I/O failure. | While one independent process holds a lock, does another `File::try_lock` on the same file return the dedicated busy state? | PASS |
| P2 | Workspace locks are independent by lock-file identity. | While Workspace A is locked, can the same process acquire Workspace B's distinct lock file? | PASS |
| P3 | A crashed/exited writer cannot leave a stale logical lock. | After killing the holder without calling `unlock`, can another process acquire the same file? | PASS |
| P4 | Separate descriptors in one process are not safely reentrant. | Does a second independently opened descriptor contend while the first descriptor in the same process holds the lock? | PASS |
| P5 | Rust 1.94 exposes one typed, portable contract for Unix and Windows. | Is `File::try_lock` stable, nonblocking, exclusive, released on close, and mapped to `TryLockError::WouldBlock` on the supported OS implementations? | PASS |
| N1 | The feature's lock-file name and owning module. | N/A — placement is a design decision, not existing-system behavior. | N/A — design decision |
| N2 | Contention latency and production load budget. | N/A — performance target belongs to falsifiable design/checkpoint. | N/A — design/checkpoint target |

## Data

- Source: production-shaped generated data.
- Shape: two temporary directories/files model two canonical Workspace lock identities; independent holder and contender processes model concurrent CLI/MCP processes; one holder is killed before cleanup.
- Safety: both probe and oracle create unique directories beneath the OS temporary directory and remove them; no repository Workspace, tracker file, or production state is opened or mutated.
- Approval: N/A — safe production-shaped data; no snapshot required.

## Probe

- File: `probe.rs`
- Mechanism: a standalone Rust 1.94 program opens lock files read/write, uses `std::fs::File::try_lock`, synchronizes with a child holder through stdout, checks same/different files, kills the holder, and checks a second same-process descriptor.
- Run: `rustc --edition 2024 .rivets-j13o/probe.rs -o /tmp/rivets-j13o-probe && /tmp/rivets-j13o-probe`
- Output:

```text
same_workspace=BUSY
different_workspace=ACQUIRED
after_holder_exit=ACQUIRED
same_process_second_fd=BUSY
```

## Oracle

- File: `oracle.py`
- Mechanism: Python calls the Linux `fcntl.flock` interface directly rather than Rust's standard-library wrapper; subprocess/descriptor lifecycle and result classification are independently implemented. For P5, the checked-in Rust 1.94 standard-library source is inspected directly: public API/docs in `library/std/src/fs.rs`, Unix implementation in `library/std/src/sys/fs/unix.rs`, and Windows implementation in `library/std/src/sys/fs/windows.rs`.
- Run: `python3 .rivets-j13o/oracle.py`
- Source audit: `grep TryLockError /home/dwalleck/.rustup/toolchains/1.94.0-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/std/src`
- Output:

```text
same_workspace=BUSY
different_workspace=ACQUIRED
after_holder_exit=ACQUIRED
same_process_second_fd=BUSY
```

- P5 source result: `File::try_lock` and `TryLockError` are stable since Rust 1.89; the API returns `WouldBlock` for contention, documents release when all duplicated/inherited handles close, uses `flock(LOCK_EX | LOCK_NB)` on supported Unix targets, and uses `LockFileEx(LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY)` with `ERROR_LOCK_VIOLATION` mapped to `WouldBlock` on Windows. A lock file must be opened read/write for the Windows contract.

## Comparisons

| ID | Probe output | Oracle output | Verdict |
|----|--------------|---------------|---------|
| P1 | Same Workspace: `BUSY` | Same Workspace: `BUSY` | PASS |
| P2 | Different Workspace: `ACQUIRED` | Different Workspace: `ACQUIRED` | PASS |
| P3 | After killed holder: `ACQUIRED` | After killed holder: `ACQUIRED` | PASS |
| P4 | Same-process second descriptor: `BUSY` | Same-process second descriptor: `BUSY` | PASS |
| P5 | Rust 1.94 probe compiles and classifies contention as `TryLockError::WouldBlock` | Current standard-library docs/source expose the stable typed API and equivalent nonblocking Unix/Windows mappings | PASS |

## Validated / learned

- P1: learned that `File::try_lock` returns `std::fs::TryLockError`, not `io::Error`; `WouldBlock` is a dedicated enum variant and other I/O remains causal in `TryLockError::Error`.
- P2: validated prior understanding that distinct lock files permit independent Workspace writers.
- P3: validated prior understanding that process/handle closure releases the OS lock without deleting or repairing a stale marker.
- P4: learned that a separately opened descriptor in the same Linux process contends; production must avoid nested acquisition and keep the existing per-Workspace in-process serialization.
- P5: validated that the repository's Rust 1.94 floor can use the standard library without a new dependency, with explicit read/write open mode and exhaustive typed error mapping.

## Related issues

- Consulted: `rivets-j13o` (this task), parent `rivets-5mlg`, dependent `rivets-8rj9`, and closed predecessor `rivets-omq0` (stale-cache overwrite protection). `rivets-5vz` discusses an Automerge alternative but does not cover this lock contract.
- Filed: none — the probe found no underlying-system defect or uncovered future-work requirement.
