//! Architecture-analysis storage layer.
//!
//! Owns the four `arch_*` schema objects and the queries that read and write them.
//! Wired into the indexing pipeline by `Tethys::run_architecture_phase`.

use crate::types::PackageSource;

use super::Index;

/// Insert payload for `repopulate_architecture`.
#[allow(dead_code)] // consumed by Task 5
pub struct PackageInsert<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub source: PackageSource,
}

impl Index {
    // Methods will be added in subsequent tasks (Tasks 5-8).
}
