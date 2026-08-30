# Review decisions

| finding-id | finding | reviewer | evidence-state | evidence | decision | fix | note |
|---|---|---|---|---|---|---|---|
| F1 | The C0 named mutation widens graph visibility before expecting Rust privacy to reject the CLI bypass; preserve the privacy boundary and make the compile-fail experiment explicit. | OMP code review for PR #99 | Verified | A temporary `use crate::storage::in_memory::inner::InMemoryStorageInner;` in `cli/execute.rs` made `cargo check -p rivets` fail with E0603 because `inner` is private. The existing design/plan instead instructed the mutation to widen visibility first. | Accept | Correct the C0 design and Slice 2 named mutation so the deliberate CLI access leaves visibility unchanged, expects E0603 from `cargo check -p rivets`, and restores to a green check. | Non-behavioral verification repair; no checkpoint or tracker issue required. |
