//! Architecture-analysis storage layer.
//!
//! Owns the four `arch_*` schema objects and the queries that read and write them.
//! Wired into the indexing pipeline by `Tethys::run_architecture_phase`.

use std::collections::HashMap;

use rusqlite::params;
use tracing::trace;

use super::Index;
use crate::error::Result;
use crate::types::{ArchStats, CouplingMetrics, CouplingSort, FileId, Package, PackageId, PackageSource};

/// Insert payload for `repopulate_architecture`.
#[allow(dead_code)] // consumed by Tasks 5-8; used in tests
pub struct PackageInsert<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub source: PackageSource,
}

impl Index {
    /// Rebuild every `arch_*` table from the supplied package list and file
    /// mappings, plus the current state of `file_deps`. Single transaction.
    ///
    /// Idempotent: identical input produces identical state.
    ///
    /// `file_to_package_name` entries whose name is not present in `packages`
    /// are silently skipped (logged at `trace!`); this lets callers feed
    /// best-effort name lookups without pre-filtering.
    ///
    /// # Errors
    /// Returns an error if `packages` contains duplicate names (violates the
    /// UNIQUE constraint on `arch_packages.name`). Callers must de-duplicate
    /// before calling.
    #[allow(dead_code)] // called from tests; wired into pipeline in Task 10
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
        {
            let mut stmt = tx.prepare(
                "INSERT INTO arch_packages (name, path, source) VALUES (?1, ?2, ?3)",
            )?;
            for pkg in packages {
                stmt.execute(params![pkg.name, pkg.path, pkg.source.as_str()])?;
            }
        }
        let packages_recorded = packages.len();

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

    /// Return every package row, ordered alphabetically by name for determinism.
    /// Unknown `source` values produce a `warn!` and are skipped.
    #[allow(dead_code)] // called from tests; wired into pipeline in Task 11
    pub fn get_packages(&self) -> Result<Vec<Package>> {
        use std::path::PathBuf;

        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, path, source FROM arch_packages ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, name, path, source_str) = row?;
            let Some(source) = PackageSource::parse(&source_str) else {
                tracing::warn!(
                    package_name = %name,
                    source = %source_str,
                    "skipping package with unknown source value"
                );
                continue;
            };
            out.push(Package {
                id: PackageId::from(id),
                name,
                path: PathBuf::from(path),
                source,
            });
        }
        Ok(out)
    }

    /// Coupling metrics for every package, sorted per the requested key.
    /// Sort is delegated to `SQLite` via `ORDER BY`.
    ///
    /// The ORDER BY fragment is chosen from a fixed set of SQL strings derived
    /// from `CouplingSort` — no user input ever reaches the SQL, so there is
    /// no injection risk.
    #[allow(dead_code)] // called from tests; wired into pipeline in Task 11
    pub fn get_coupling_metrics(&self, sort: CouplingSort) -> Result<Vec<CouplingMetrics>> {
        use std::path::PathBuf;

        let order_clause = match sort {
            CouplingSort::Instability => "c.instability DESC, p.name ASC",
            CouplingSort::Afferent => "c.afferent DESC, p.name ASC",
            CouplingSort::Efferent => "c.efferent DESC, p.name ASC",
            CouplingSort::Name => "p.name ASC",
        };

        let sql = format!(
            "SELECT p.id, p.name, p.path, p.source,
                    c.afferent, c.efferent, c.instability
             FROM arch_coupling c
             JOIN arch_packages p ON p.id = c.package_id
             ORDER BY {order_clause}"
        );

        let conn = self.connection()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, f64>(6)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, name, path, source_str, ca, ce, instability) = row?;
            let Some(source) = PackageSource::parse(&source_str) else {
                tracing::warn!(
                    package_name = %name,
                    source = %source_str,
                    "skipping coupling row with unknown source"
                );
                continue;
            };
            out.push(CouplingMetrics {
                package: Package {
                    id: PackageId::from(id),
                    name,
                    path: PathBuf::from(path),
                    source,
                },
                afferent: u32::try_from(ca).unwrap_or(u32::MAX),
                efferent: u32::try_from(ce).unwrap_or(u32::MAX),
                instability,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod get_packages_tests {
    use super::*;
    use tempfile::TempDir;

    fn seeded_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("idx.db");
        let index = Index::open(&path).expect("open");
        let packages = [
            PackageInsert { name: "z_crate", path: "crates/z", source: PackageSource::Manifest },
            PackageInsert { name: "a_crate", path: "crates/a", source: PackageSource::Manifest },
        ];
        index.repopulate_architecture(&packages, &[]).expect("repopulate");
        (dir, index)
    }

    #[test]
    fn get_packages_returns_alphabetical_by_name() {
        let (_dir, index) = seeded_index();
        let pkgs = index.get_packages().expect("get_packages");
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "a_crate");
        assert_eq!(pkgs[1].name, "z_crate");
    }

    #[test]
    fn get_packages_decodes_source_field() {
        let (_dir, index) = seeded_index();
        let pkgs = index.get_packages().expect("get_packages");
        assert!(pkgs.iter().all(|p| p.source == PackageSource::Manifest));
    }

    #[test]
    fn get_packages_empty_for_fresh_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        assert!(index.get_packages().expect("get_packages").is_empty());
    }
}

#[cfg(test)]
mod coupling_metrics_tests {
    use super::*;
    use crate::types::{CouplingSort, Language};
    use std::path::Path;
    use tempfile::TempDir;

    /// Three-crate fixture: a → b, a → c, b → c.
    fn seeded_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f_a = index.upsert_file(Path::new("a/lib.rs"), Language::Rust, 0, 0, None).expect("a");
        let f_b = index.upsert_file(Path::new("b/lib.rs"), Language::Rust, 0, 0, None).expect("b");
        let f_c = index.upsert_file(Path::new("c/lib.rs"), Language::Rust, 0, 0, None).expect("c");

        index.insert_file_dependency(f_a, f_b).expect("a→b");
        index.insert_file_dependency(f_a, f_c).expect("a→c");
        index.insert_file_dependency(f_b, f_c).expect("b→c");

        let packages = [
            PackageInsert { name: "a", path: "a", source: PackageSource::Manifest },
            PackageInsert { name: "b", path: "b", source: PackageSource::Manifest },
            PackageInsert { name: "c", path: "c", source: PackageSource::Manifest },
        ];
        let mappings = [(f_a, "a"), (f_b, "b"), (f_c, "c")];
        index.repopulate_architecture(&packages, &mappings).expect("repopulate");

        (dir, index)
    }

    fn metrics_for<'a>(rows: &'a [CouplingMetrics], name: &str) -> &'a CouplingMetrics {
        rows.iter()
            .find(|m| m.package.name == name)
            .unwrap_or_else(|| panic!("no row for {name}"))
    }

    #[test]
    fn coupling_metrics_match_expected_ca_ce_instability() {
        let (_dir, index) = seeded_index();
        let rows = index.get_coupling_metrics(CouplingSort::Name).expect("metrics");

        let a = metrics_for(&rows, "a");
        assert_eq!((a.afferent, a.efferent), (0, 2));
        assert!((a.instability - 1.0).abs() < 1e-9);

        let b = metrics_for(&rows, "b");
        assert_eq!((b.afferent, b.efferent), (1, 1));
        assert!((b.instability - 0.5).abs() < 1e-9);

        let c = metrics_for(&rows, "c");
        assert_eq!((c.afferent, c.efferent), (2, 0));
        assert!((c.instability - 0.0).abs() < 1e-9);
    }

    #[test]
    fn sort_by_instability_descending() {
        let (_dir, index) = seeded_index();
        let rows = index.get_coupling_metrics(CouplingSort::Instability).expect("metrics");
        assert_eq!(rows[0].package.name, "a");
        assert_eq!(rows[1].package.name, "b");
        assert_eq!(rows[2].package.name, "c");
    }

    #[test]
    fn sort_by_name_ascending() {
        let (_dir, index) = seeded_index();
        let rows = index.get_coupling_metrics(CouplingSort::Name).expect("metrics");
        assert_eq!(rows[0].package.name, "a");
        assert_eq!(rows[1].package.name, "b");
        assert_eq!(rows[2].package.name, "c");
    }

    #[test]
    fn isolated_package_has_zero_instability() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        let packages = [PackageInsert {
            name: "lonely", path: "lonely", source: PackageSource::Manifest,
        }];
        index.repopulate_architecture(&packages, &[]).expect("repopulate");

        let rows = index.get_coupling_metrics(CouplingSort::Name).expect("metrics");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].afferent, rows[0].efferent), (0, 0));
        assert!(rows[0].instability.abs() < 1e-9);
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
