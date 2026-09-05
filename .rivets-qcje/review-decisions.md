# Parentage review decisions

| finding-id | finding | reviewer | evidence-state | evidence | decision | fix | note |
|---|---|---|---|---|---|---|---|
| F1 | Valid reverse Parentage/Blocking edges disappear on restart; use kind-specific reload cycle checks. | SpecReview | Verified | Real CLI created Blocking parent→child and Parentage child→parent; fresh list returned [] with CircularDependency. | Modify | Preserve upstream kind-specific loader during rebase; add a mixed-edge restart regression and prove its sensitivity. | C3/C9; this is valid data, not deferred legacy repair. |
| F2 | Completion commit overwrites unrelated Issue states/Notes and deletes omq0; restore history. | Main + SpecReview | Verified | 18c0f8d reopens brai/j13o, removes three Notes and assignees, deletes omq0. | Accept | Reconstruct each replayed tracker change from current upstream plus qcje-only record changes. | Keep every unrelated upstream record byte-for-byte. |
| F3 | CLI reimplements NoParent and disagrees with MCP on self-move errors; delegate one storage transition. | StandardsReview + SpecReview | Verified | execute_parent preflights parent_of before Parentage::new; real CLI self-move returns NoParent while MCP constructs SelfReference first. | Modify | Make move_parent return the previous Parentage; construct request once in each adapter and let storage decide NoParent; preserve adapter success payloads. | C4/C10/C11; no new public result type is needed. |
| F4 | Parentage branch retains obsolete stack base and conflicts with current main; reconcile actual five commits. | Main | Verified | git merge-tree main work/qcje reported 22 conflicted paths; original plan requires default-branch reconciliation. | Accept | Replay only 2b7e4be..18c0f8d onto fetched origin/main cc0b2ad; preserve current Assignment and relationship contracts. | Original tip retained at backup/qcje-pre-review-20260905; no push/merge to main. |

## Slice 5 checkpoint — F1, with reconciliation F2/F4

| Gate | Result |
|---|---|
| Affected tests | PASS — mixed-kind restart regression and affected Parentage suites. |
| Pending falsifier | PASS — the real CLI reverse-kind reproduction now retains both edges across process restart. |
| Stress fixture | PASS — both JSONL record orders and second save/reload preserve exact Parentage, Blocking, Ready, and Blocked results. |
| Independent oracle | PASS — literal child/parent and dependent/prerequisite pairs; child Ready, parent Blocked. |
| Production-scale budget | N/A — no production loop or runtime phase changed; upstream kind-filtered loader retained. |
| Regression fence | PASS — `parentage_reverse_blocking_survives_restart`. |
| Named mutation | PASS — removing the edge-kind filter produced the expected false `CircularDependency` and failed the fence. |
| Restored fence | PASS — restoring the filter returned the fence to green. |

F2: exact Issue-ID keyed comparison confirmed all 261 unrelated tracker records
match fetched upstream byte-for-byte, including brai, j13o, and omq0.
F4: only five Parentage commits were replayed onto `cc0b2ad`; dropped conflict
delimiters were repaired, existing Assignment/Related/Discovery behavior retained,
and the four Parentage operations were classified in the current parity registry.
The registry's broader ID-parsing and parent-and-direct-children roadmap targets
remain explicitly classified rather than falsely marked conformant.
