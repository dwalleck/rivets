//! Architecture-analysis storage layer.
//!
//! Owns the four `arch_*` schema objects and the queries that read and write them.
//! Wired into the indexing pipeline by `Tethys::run_architecture_phase`.

use std::collections::HashMap;

use rusqlite::params;
use tracing::trace;

use super::Index;
use crate::error::Result;
use crate::types::{ArchStats, FileId, PackageId, PackageSource};

/// Insert payload for `repopulate_architecture`.
#[allow(dead_code)] // consumed by Tasks 5-8; used in tests
pub struct PackageInsert<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub source: PackageSource,
}

impl Index {
    #[allow(dead_code)] // called from tests; wired into pipeline in Task 10
    /// Rebuild every `arch_*` table from the supplied package list and file
    /// mappings, plus the current state of `file_deps`. Single transaction.
    ///
    /// Idempotent: identical input produces identical state.
    ///
    /// `file_to_package_name` entries whose name is not present in `packages`
    /// are silently skipped (logged at `trace!`); this lets callers feed
    /// best-effort name lookups without pre-filtering.
    pub fn repopulate_architecture(
        &self,
        packages: &[PackageInsert<'_>],
        file_to_package_name: &[(FileId, &str)],
    ) -> Result<ArchStats> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;

        // 1. Wipe. Cascade clears the two child tables.
        tx.execute("DELETE FROM arch_packages", [])?;

        // 2. Insert packages.
        let mut packages_recorded: usize = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO arch_packages (name, path, source) VALUES (?1, ?2, ?3)",
            )?;
            for pkg in packages {
                stmt.execute(params![pkg.name, pkg.path, pkg.source.as_str()])?;
                packages_recorded += 1;
            }
        }

        // 3. Read back name → id map (needed to translate file mappings).
        let mut name_to_id: HashMap<String, PackageId> = HashMap::new();
        {
            let mut stmt = tx.prepare("SELECT id, name FROM arch_packages")?;
            let rows = stmt.query_map([], |row| {
                Ok((PackageId::from(row.get::<_, i64>(0)?), row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, name) = row?;
                name_to_id.insert(name, id);
            }
        }

        // 4. Insert file → package mappings, skipping unknown names.
        let mut files_assigned: usize = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO arch_file_packages (file_id, package_id) VALUES (?1, ?2)",
            )?;
            for (file_id, name) in file_to_package_name {
                if let Some(pkg_id) = name_to_id.get(*name) {
                    stmt.execute(params![file_id.as_i64(), pkg_id.as_i64()])?;
                    files_assigned += 1;
                } else {
                    trace!(
                        file_id = file_id.as_i64(),
                        package_name = %name,
                        "skipping file with unknown package name"
                    );
                }
            }
        }

        // 5. Roll up cross-package edges.
        let package_deps_recorded = tx.execute(
            "INSERT INTO arch_package_deps (source_pkg, target_pkg, dep_count)
             SELECT sp.package_id, tp.package_id, COUNT(*)
             FROM file_deps fd
             JOIN arch_file_packages sp ON sp.file_id = fd.from_file_id
             JOIN arch_file_packages tp ON tp.file_id = fd.to_file_id
             WHERE sp.package_id <> tp.package_id
             GROUP BY sp.package_id, tp.package_id",
            [],
        )?;

        tx.commit()?;

        Ok(ArchStats {
            packages_recorded,
            files_assigned,
            package_deps_recorded,
        })
    }
}

#[cfg(test)]
mod repopulate_tests {
    use super::*;
    use crate::types::Language;
    use std::path::Path;
    use tempfile::TempDir;

    fn temp_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("idx.db");
        let index = Index::open(&path).expect("open index");
        (dir, index)
    }

    /// Inserts a file and returns its `FileId`.
    fn add_file(index: &mut Index, rel_path: &str) -> FileId {
        index
            .upsert_file(Path::new(rel_path), Language::Rust, 0, 0, None)
            .expect("upsert file")
    }

    #[test]
    fn repopulate_architecture_inserts_packages() {
        let (_dir, index) = temp_index();

        let packages = [
            PackageInsert {
                name: "crate_a",
                path: "crate_a",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "crate_b",
                path: "crate_b",
                source: PackageSource::Manifest,
            },
        ];

        let stats = index
            .repopulate_architecture(&packages, &[])
            .expect("repopulate");

        assert_eq!(stats.packages_recorded, 2);
        assert_eq!(stats.files_assigned, 0);
        assert_eq!(stats.package_deps_recorded, 0);
    }

    #[test]
    fn repopulate_architecture_assigns_files_and_deps() {
        let (_dir, mut index) = temp_index();

        let f_a = add_file(&mut index, "crate_a/lib.rs");
        let f_b = add_file(&mut index, "crate_b/lib.rs");
        let f_c = add_file(&mut index, "crate_c/lib.rs");

        // crate_a depends on crate_b and crate_c; crate_b depends on crate_c.
        index.insert_file_dependency(f_a, f_b).expect("dep a→b");
        index.insert_file_dependency(f_a, f_c).expect("dep a→c");
        index.insert_file_dependency(f_b, f_c).expect("dep b→c");

        let packages = [
            PackageInsert {
                name: "crate_a",
                path: "crate_a",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "crate_b",
                path: "crate_b",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "crate_c",
                path: "crate_c",
                source: PackageSource::Manifest,
            },
        ];

        let mappings = [(f_a, "crate_a"), (f_b, "crate_b"), (f_c, "crate_c")];

        let stats = index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        assert_eq!(stats.packages_recorded, 3);
        assert_eq!(stats.files_assigned, 3);
        assert_eq!(stats.package_deps_recorded, 3, "a→b, a→c, b→c");
    }

    #[test]
    fn repopulate_architecture_is_idempotent() {
        let (_dir, mut index) = temp_index();
        let f = add_file(&mut index, "crate_a/lib.rs");
        let packages = [PackageInsert {
            name: "crate_a",
            path: "crate_a",
            source: PackageSource::Manifest,
        }];
        let mappings = [(f, "crate_a")];

        let s1 = index
            .repopulate_architecture(&packages, &mappings)
            .expect("first");
        let s2 = index
            .repopulate_architecture(&packages, &mappings)
            .expect("second");

        assert_eq!(s1, s2);
    }

    #[test]
    fn repopulate_architecture_skips_unknown_package_names() {
        let (_dir, mut index) = temp_index();
        let f = add_file(&mut index, "orphan.rs");
        let packages = [PackageInsert {
            name: "crate_a",
            path: "crate_a",
            source: PackageSource::Manifest,
        }];
        // file_to_package_name references a package not in `packages`.
        let mappings = [(f, "missing_crate")];

        let stats = index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        assert_eq!(stats.packages_recorded, 1);
        assert_eq!(stats.files_assigned, 0, "unknown name skipped");
    }
}
