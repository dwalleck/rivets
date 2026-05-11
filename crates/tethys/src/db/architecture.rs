//! Architecture-analysis storage layer.
//!
//! Owns the four `arch_*` schema objects and the queries that read and write them.
//! Wired into the indexing pipeline by `Tethys::run_architecture_phase`.

use std::collections::HashMap;

use rusqlite::params;
use tracing::trace;

use super::Index;
use crate::error::Result;
use crate::types::{
    ArchStats, CouplingDetail, CouplingMetrics, CouplingSort, FileId, Package, PackageDependency,
    PackageId, PackageSource,
};

/// Insert payload for `repopulate_architecture`.
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
    pub fn repopulate_architecture(
        &self,
        packages: &[PackageInsert<'_>],
        file_to_package_name: &[(FileId, &str)],
    ) -> Result<ArchStats> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;

        // 1. Wipe. Cascade clears the two child tables.
        tx.execute("DELETE FROM arch_packages", [])?;

        // 2. Insert packages; capture last_insert_rowid() instead of selecting
        //    back — avoids a round-trip query and keeps the mapping close to insertion.
        let mut name_to_id: HashMap<&str, PackageId> = HashMap::with_capacity(packages.len());
        {
            let mut stmt =
                tx.prepare("INSERT INTO arch_packages (name, path, source) VALUES (?1, ?2, ?3)")?;
            for pkg in packages {
                stmt.execute(params![pkg.name, pkg.path, pkg.source.as_str()])?;
                // Safe because (a) the transaction holds the exclusive write lock for
                // its lifetime, so no other writer interleaves, and (b) `arch_packages`
                // uses INTEGER PRIMARY KEY — `last_insert_rowid()` returns that rowid.
                // If the PK is ever migrated to a non-rowid type (e.g., UUID, text key,
                // or WITHOUT ROWID), switch back to a SELECT-back approach.
                name_to_id.insert(pkg.name, PackageId::new(tx.last_insert_rowid()));
            }
        }
        let packages_recorded = packages.len();

        // 3. Insert file → package mappings, skipping unknown names.
        let mut files_assigned: usize = 0;
        {
            let mut stmt =
                tx.prepare("INSERT INTO arch_file_packages (file_id, package_id) VALUES (?1, ?2)")?;
            for (file_id, name) in file_to_package_name {
                if let Some(pkg_id) = name_to_id.get(name) {
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

        // 4. Roll up cross-package edges.
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
    pub fn get_packages(&self) -> Result<Vec<Package>> {
        use std::path::PathBuf;

        let conn = self.connection()?;
        let mut stmt =
            conn.prepare("SELECT id, name, path, source FROM arch_packages ORDER BY name ASC")?;
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
                id: PackageId::new(id),
                name,
                path: PathBuf::from(path),
                source,
            });
        }
        Ok(out)
    }

    /// Coupling metrics for every package, sorted per the requested key.
    ///
    /// Rows are fetched unsorted from the DB. For [`CouplingSort::Instability`],
    /// the Rust-side sort calls [`CouplingMetrics::instability`], keeping the
    /// formula in a single canonical location. The other three sort variants
    /// (`Afferent`, `Efferent`, `Name`) compare the stored `afferent`/`efferent`
    /// integer fields and the `name` string directly.
    pub fn get_coupling_metrics(&self, sort: CouplingSort) -> Result<Vec<CouplingMetrics>> {
        use std::path::PathBuf;

        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, p.path, p.source,
                    c.afferent, c.efferent
             FROM arch_coupling c
             JOIN arch_packages p ON p.id = c.package_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, name, path, source_str, ca, ce) = row?;
            let Some(source) = PackageSource::parse(&source_str) else {
                tracing::warn!(
                    package_name = %name,
                    source = %source_str,
                    "skipping coupling row with unknown source"
                );
                continue;
            };
            let afferent = saturating_coupling_to_u32(ca, &name, "afferent");
            let efferent = saturating_coupling_to_u32(ce, &name, "efferent");
            out.push(CouplingMetrics {
                package: Package {
                    id: PackageId::new(id),
                    name,
                    path: PathBuf::from(path),
                    source,
                },
                afferent,
                efferent,
            });
        }

        // Sort entirely in Rust so the instability formula lives in exactly one place.
        match sort {
            CouplingSort::Instability => {
                out.sort_by(|a, b| {
                    b.instability()
                        .partial_cmp(&a.instability())
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.package.name.cmp(&b.package.name))
                });
            }
            CouplingSort::Afferent => {
                out.sort_by(|a, b| {
                    b.afferent
                        .cmp(&a.afferent)
                        .then_with(|| a.package.name.cmp(&b.package.name))
                });
            }
            CouplingSort::Efferent => {
                out.sort_by(|a, b| {
                    b.efferent
                        .cmp(&a.efferent)
                        .then_with(|| a.package.name.cmp(&b.package.name))
                });
            }
            CouplingSort::Name => {
                out.sort_by(|a, b| a.package.name.cmp(&b.package.name));
            }
        }

        Ok(out)
    }
}

/// Convert an `i64` from the DB to a `u32`, saturating at `u32::MAX` with a
/// `warn!` log when the value doesn't fit. Mirrors `lib.rs::saturating_depth_to_u32`.
fn saturating_coupling_to_u32(value: i64, package_name: &str, field: &str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| {
        tracing::warn!(
            package_name = %package_name,
            field = %field,
            requested = value,
            cap = u32::MAX,
            "coupling value exceeds u32::MAX; saturating"
        );
        u32::MAX
    })
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
            PackageInsert {
                name: "z_crate",
                path: "crates/z",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "a_crate",
                path: "crates/a",
                source: PackageSource::Manifest,
            },
        ];
        index
            .repopulate_architecture(&packages, &[])
            .expect("repopulate");
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

/// Direction used by [`Index::fetch_neighbors`] to query either dependents or
/// dependencies of a package.
#[derive(Clone, Copy, Debug)]
enum Direction {
    Outgoing,
    Incoming,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Outgoing => f.write_str("outgoing"),
            Direction::Incoming => f.write_str("incoming"),
        }
    }
}

impl Index {
    /// Detailed coupling for one package by exact name.
    ///
    /// Returns `Ok(None)` when no package matches. Returns `Err` on a DB failure
    /// or if the matched package row has an unrecognised `source` value (which
    /// represents corruption or a schema-version mismatch for the target — see
    /// the asymmetric behaviour note below).
    ///
    /// Incoming and outgoing lists are sorted by `dep_count` descending, then
    /// by name ascending.
    ///
    /// # Error asymmetry between target and neighbours
    ///
    /// Corrupt-source handling is deliberately asymmetric:
    ///
    /// - For the **target package** (this method): an unrecognised `source` is
    ///   `Err`, because the caller asked for this specific package and silently
    ///   returning `Ok(None)` would lie about its existence.
    /// - For **neighbour packages** (in `fetch_neighbors`): an unrecognised
    ///   `source` is logged at `warn!` and the neighbour is omitted from the
    ///   returned list. One corrupt neighbour does not abort the whole detail
    ///   query.
    ///
    /// A consequence: in the presence of corruption, the `incoming` /
    /// `outgoing` lists may be silently truncated. Callers that need strict
    /// integrity should monitor the `warn!` log channel from this module.
    pub fn get_package_coupling(&self, name: &str) -> Result<Option<CouplingDetail>> {
        use std::path::PathBuf;

        // Scope the connection lock so it is dropped before fetch_neighbors
        // acquires it again (the Mutex is not re-entrant).
        let row: Option<(i64, String, String, String, i64, i64)> = {
            let conn = self.connection()?;
            conn.query_row(
                "SELECT p.id, p.name, p.path, p.source,
                        c.afferent, c.efferent
                 FROM arch_coupling c
                 JOIN arch_packages p ON p.id = c.package_id
                 WHERE p.name = ?1",
                params![name],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?
        };

        let Some((id, pkg_name, pkg_path, source_str, ca, ce)) = row else {
            return Ok(None);
        };
        let Some(source) = PackageSource::parse(&source_str) else {
            return Err(crate::error::Error::Internal(format!(
                "package '{pkg_name}' has unknown source value '{source_str}'; \
                 possible schema version mismatch or external DB modification"
            )));
        };

        let target = Package {
            id: PackageId::new(id),
            name: pkg_name,
            path: PathBuf::from(pkg_path),
            source,
        };

        let afferent = saturating_coupling_to_u32(ca, &target.name, "afferent");
        let efferent = saturating_coupling_to_u32(ce, &target.name, "efferent");

        // Connection lock is released above; re-acquire for neighbor queries.
        let outgoing = self.fetch_neighbors(target.id, Direction::Outgoing)?;
        let incoming = self.fetch_neighbors(target.id, Direction::Incoming)?;

        Ok(Some(CouplingDetail {
            metrics: CouplingMetrics {
                package: target,
                afferent,
                efferent,
            },
            incoming,
            outgoing,
        }))
    }

    /// Fetch neighbouring packages in the requested direction.
    ///
    /// The two queries are spelled out as separate `const` strings rather than
    /// assembled at runtime via `format!`. This makes the column-name choice
    /// visible to a reader without relying on a "no user input reaches SQL"
    /// comment, and makes the safety property structural rather than
    /// behavioural.
    fn fetch_neighbors(
        &self,
        package_id: PackageId,
        dir: Direction,
    ) -> Result<Vec<PackageDependency>> {
        use std::path::PathBuf;

        /// SQL for "packages this package depends on" (outgoing edges).
        const OUTGOING_SQL: &str = "
            SELECT p.id, p.name, p.path, p.source, d.dep_count
            FROM arch_package_deps d
            JOIN arch_packages p ON p.id = d.target_pkg
            WHERE d.source_pkg = ?1
            ORDER BY d.dep_count DESC, p.name ASC";

        /// SQL for "packages that depend on this package" (incoming edges).
        const INCOMING_SQL: &str = "
            SELECT p.id, p.name, p.path, p.source, d.dep_count
            FROM arch_package_deps d
            JOIN arch_packages p ON p.id = d.source_pkg
            WHERE d.target_pkg = ?1
            ORDER BY d.dep_count DESC, p.name ASC";

        let sql = match dir {
            Direction::Outgoing => OUTGOING_SQL,
            Direction::Incoming => INCOMING_SQL,
        };

        let conn = self.connection()?;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![package_id.as_i64()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, name, path, source_str, dep_count) = row?;
            let Some(source) = PackageSource::parse(&source_str) else {
                tracing::warn!(
                    package_name = %name,
                    source = %source_str,
                    direction = %dir,
                    "neighbor package has unknown source value; omitting from results"
                );
                continue;
            };
            let dep_count_u32 = saturating_coupling_to_u32(dep_count, &name, "dep_count");
            out.push(PackageDependency {
                package: Package {
                    id: PackageId::new(id),
                    name,
                    path: PathBuf::from(path),
                    source,
                },
                dep_count: dep_count_u32,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod package_coupling_tests {
    use super::*;
    use crate::types::Language;
    use std::path::Path;
    use tempfile::TempDir;
    use tracing_test::traced_test;

    fn seeded_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f_a = index
            .upsert_file(Path::new("a/lib.rs"), Language::Rust, 0, 0, None)
            .expect("a");
        let f_b = index
            .upsert_file(Path::new("b/lib.rs"), Language::Rust, 0, 0, None)
            .expect("b");
        let f_c = index
            .upsert_file(Path::new("c/lib.rs"), Language::Rust, 0, 0, None)
            .expect("c");

        index.insert_file_dependency(f_a, f_b).expect("a→b");
        index.insert_file_dependency(f_a, f_c).expect("a→c");
        index.insert_file_dependency(f_b, f_c).expect("b→c");

        let packages = [
            PackageInsert {
                name: "a",
                path: "a",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "b",
                path: "b",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "c",
                path: "c",
                source: PackageSource::Manifest,
            },
        ];
        let mappings = [(f_a, "a"), (f_b, "b"), (f_c, "c")];
        index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");
        (dir, index)
    }

    #[test]
    fn package_coupling_returns_outgoing_and_incoming() {
        let (_dir, index) = seeded_index();
        let detail = index
            .get_package_coupling("b")
            .expect("query")
            .expect("found");

        assert_eq!(detail.metrics.package.name, "b");
        assert_eq!((detail.metrics.afferent, detail.metrics.efferent), (1, 1));

        let in_names: Vec<_> = detail
            .incoming
            .iter()
            .map(|d| d.package.name.as_str())
            .collect();
        assert_eq!(in_names, ["a"]);

        let out_names: Vec<_> = detail
            .outgoing
            .iter()
            .map(|d| d.package.name.as_str())
            .collect();
        assert_eq!(out_names, ["c"]);
    }

    #[test]
    fn package_coupling_none_for_missing_package() {
        let (_dir, index) = seeded_index();
        assert!(
            index
                .get_package_coupling("does-not-exist")
                .expect("query")
                .is_none()
        );
    }

    #[test]
    fn package_coupling_for_isolated_package_returns_empty_lists() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        let packages = [PackageInsert {
            name: "lonely",
            path: "lonely",
            source: PackageSource::Manifest,
        }];
        index
            .repopulate_architecture(&packages, &[])
            .expect("repopulate");

        let detail = index
            .get_package_coupling("lonely")
            .expect("query")
            .expect("found");
        assert!(detail.incoming.is_empty());
        assert!(detail.outgoing.is_empty());
        assert!(detail.metrics.instability().abs() < 1e-9);
    }

    #[test]
    fn package_coupling_dep_count_aggregates_multiple_file_edges() {
        // a has TWO files, both depending on b's single file.
        // Expected: arch_package_deps(a, b).dep_count == 2.
        let dir = tempfile::tempdir().expect("temp dir");
        let mut index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f_a1 = index
            .upsert_file(Path::new("a/lib.rs"), Language::Rust, 0, 0, None)
            .expect("a1");
        let f_a2 = index
            .upsert_file(Path::new("a/helpers.rs"), Language::Rust, 0, 0, None)
            .expect("a2");
        let f_b = index
            .upsert_file(Path::new("b/lib.rs"), Language::Rust, 0, 0, None)
            .expect("b");

        index.insert_file_dependency(f_a1, f_b).expect("a1→b");
        index.insert_file_dependency(f_a2, f_b).expect("a2→b");

        let packages = [
            PackageInsert {
                name: "a",
                path: "a",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "b",
                path: "b",
                source: PackageSource::Manifest,
            },
        ];
        let mappings = [(f_a1, "a"), (f_a2, "a"), (f_b, "b")];
        index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        let detail = index
            .get_package_coupling("a")
            .expect("query")
            .expect("found");
        assert_eq!(detail.outgoing.len(), 1, "single edge to b");
        assert_eq!(detail.outgoing[0].package.name, "b");
        assert_eq!(
            detail.outgoing[0].dep_count, 2,
            "two file-edges roll up to dep_count=2"
        );
    }

    #[test]
    fn repopulate_architecture_filters_intra_package_deps() {
        // Two files in the same package, with a file_dep between them.
        // The dep must NOT appear in arch_package_deps (no self-edges allowed).
        let dir = tempfile::tempdir().expect("temp dir");
        let mut index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f1 = index
            .upsert_file(Path::new("a/lib.rs"), Language::Rust, 0, 0, None)
            .expect("f1");
        let f2 = index
            .upsert_file(Path::new("a/helpers.rs"), Language::Rust, 0, 0, None)
            .expect("f2");
        index.insert_file_dependency(f1, f2).expect("f1→f2");

        let packages = [PackageInsert {
            name: "a",
            path: "a",
            source: PackageSource::Manifest,
        }];
        let mappings = [(f1, "a"), (f2, "a")];
        let stats = index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        assert_eq!(
            stats.package_deps_recorded, 0,
            "intra-package dep filtered out"
        );

        let detail = index
            .get_package_coupling("a")
            .expect("query")
            .expect("found");
        assert!(detail.outgoing.is_empty());
        assert!(detail.incoming.is_empty());
        assert_eq!(detail.metrics.afferent, 0);
        assert_eq!(detail.metrics.efferent, 0);
    }

    #[test]
    fn fetch_neighbors_sorts_by_dep_count_desc_then_name_asc() {
        // Fixture: package "hub" has three outgoing neighbors with varying edge counts.
        //   alpha  — 1 edge   (ties with gamma; alpha < gamma alphabetically)
        //   beta   — 3 edges  (highest dep_count → first)
        //   gamma  — 1 edge   (ties with alpha; gamma > alpha alphabetically)
        //
        // Expected outgoing order: [beta, alpha, gamma].
        let dir = tempfile::tempdir().expect("temp dir");
        let mut index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f_hub_a = index
            .upsert_file(Path::new("hub/a.rs"), Language::Rust, 0, 0, None)
            .expect("hub a");
        let f_hub_b = index
            .upsert_file(Path::new("hub/b.rs"), Language::Rust, 0, 0, None)
            .expect("hub b");
        let f_hub_c = index
            .upsert_file(Path::new("hub/c.rs"), Language::Rust, 0, 0, None)
            .expect("hub c");
        let f_alpha = index
            .upsert_file(Path::new("alpha/lib.rs"), Language::Rust, 0, 0, None)
            .expect("alpha");
        let f_beta_1 = index
            .upsert_file(Path::new("beta/lib.rs"), Language::Rust, 0, 0, None)
            .expect("beta 1");
        let f_beta_2 = index
            .upsert_file(Path::new("beta/helpers.rs"), Language::Rust, 0, 0, None)
            .expect("beta 2");
        let f_beta_3 = index
            .upsert_file(Path::new("beta/util.rs"), Language::Rust, 0, 0, None)
            .expect("beta 3");
        let f_gamma = index
            .upsert_file(Path::new("gamma/lib.rs"), Language::Rust, 0, 0, None)
            .expect("gamma");

        // hub → alpha: 1 file edge
        index
            .insert_file_dependency(f_hub_a, f_alpha)
            .expect("hub→alpha");
        // hub → beta: 3 file edges
        index
            .insert_file_dependency(f_hub_a, f_beta_1)
            .expect("hub→beta1");
        index
            .insert_file_dependency(f_hub_b, f_beta_2)
            .expect("hub→beta2");
        index
            .insert_file_dependency(f_hub_c, f_beta_3)
            .expect("hub→beta3");
        // hub → gamma: 1 file edge
        index
            .insert_file_dependency(f_hub_a, f_gamma)
            .expect("hub→gamma");

        let packages = [
            PackageInsert {
                name: "hub",
                path: "hub",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "alpha",
                path: "alpha",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "beta",
                path: "beta",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "gamma",
                path: "gamma",
                source: PackageSource::Manifest,
            },
        ];
        let mappings = [
            (f_hub_a, "hub"),
            (f_hub_b, "hub"),
            (f_hub_c, "hub"),
            (f_alpha, "alpha"),
            (f_beta_1, "beta"),
            (f_beta_2, "beta"),
            (f_beta_3, "beta"),
            (f_gamma, "gamma"),
        ];
        index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        let detail = index
            .get_package_coupling("hub")
            .expect("query")
            .expect("found");
        let names: Vec<_> = detail
            .outgoing
            .iter()
            .map(|d| d.package.name.as_str())
            .collect();
        let counts: Vec<_> = detail.outgoing.iter().map(|d| d.dep_count).collect();

        assert_eq!(
            names,
            ["beta", "alpha", "gamma"],
            "dep_count DESC then name ASC"
        );
        assert_eq!(
            counts,
            [3, 1, 1],
            "beta has 3 edges, alpha and gamma have 1 each"
        );
    }

    #[test]
    fn fetch_neighbors_sorts_incoming_by_dep_count_desc_then_name_asc() {
        // Fixture: package "hub" has three packages depending on it with varying edge counts.
        //   alpha → hub: 1 edge   (tied with gamma; alpha < gamma alphabetically)
        //   beta  → hub: 3 edges  (highest dep_count → first)
        //   gamma → hub: 1 edge   (tied with alpha)
        //
        // Expected incoming order: [beta, alpha, gamma].
        let dir = tempfile::tempdir().expect("temp dir");
        let mut index = Index::open(&dir.path().join("idx.db")).expect("open");

        let f_hub = index
            .upsert_file(Path::new("hub/lib.rs"), Language::Rust, 0, 0, None)
            .expect("hub");
        let f_alpha = index
            .upsert_file(Path::new("alpha/lib.rs"), Language::Rust, 0, 0, None)
            .expect("alpha");
        let f_beta_1 = index
            .upsert_file(Path::new("beta/a.rs"), Language::Rust, 0, 0, None)
            .expect("beta 1");
        let f_beta_2 = index
            .upsert_file(Path::new("beta/b.rs"), Language::Rust, 0, 0, None)
            .expect("beta 2");
        let f_beta_3 = index
            .upsert_file(Path::new("beta/c.rs"), Language::Rust, 0, 0, None)
            .expect("beta 3");
        let f_gamma = index
            .upsert_file(Path::new("gamma/lib.rs"), Language::Rust, 0, 0, None)
            .expect("gamma");

        // alpha → hub: 1 edge
        index
            .insert_file_dependency(f_alpha, f_hub)
            .expect("alpha→hub");
        // beta → hub: 3 edges (three files in beta each reference hub)
        index
            .insert_file_dependency(f_beta_1, f_hub)
            .expect("beta1→hub");
        index
            .insert_file_dependency(f_beta_2, f_hub)
            .expect("beta2→hub");
        index
            .insert_file_dependency(f_beta_3, f_hub)
            .expect("beta3→hub");
        // gamma → hub: 1 edge
        index
            .insert_file_dependency(f_gamma, f_hub)
            .expect("gamma→hub");

        let packages = [
            PackageInsert {
                name: "hub",
                path: "hub",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "alpha",
                path: "alpha",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "beta",
                path: "beta",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "gamma",
                path: "gamma",
                source: PackageSource::Manifest,
            },
        ];
        let mappings = [
            (f_hub, "hub"),
            (f_alpha, "alpha"),
            (f_beta_1, "beta"),
            (f_beta_2, "beta"),
            (f_beta_3, "beta"),
            (f_gamma, "gamma"),
        ];
        index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

        let detail = index
            .get_package_coupling("hub")
            .expect("query")
            .expect("found");
        let names: Vec<_> = detail
            .incoming
            .iter()
            .map(|d| d.package.name.as_str())
            .collect();
        let counts: Vec<_> = detail.incoming.iter().map(|d| d.dep_count).collect();

        assert_eq!(
            names,
            ["beta", "alpha", "gamma"],
            "dep_count DESC then name ASC"
        );
        assert_eq!(
            counts,
            [3, 1, 1],
            "beta has 3 incoming, alpha and gamma have 1 each"
        );
    }

    #[test]
    fn get_package_coupling_returns_err_for_corrupt_target_source() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("idx.db");

        // Open via Index to apply schema, then drop and reopen raw to bypass CHECK.
        {
            let _ = Index::open(&db_path).expect("schema");
        }
        let raw = Connection::open(&db_path).expect("raw open");
        raw.execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO arch_packages (id, name, path, source)
             VALUES (1, 'broken', 'broken', 'totally-bogus');",
        )
        .expect("inject corrupt row");
        drop(raw);

        // Now query via Index — should return Err.
        let index = Index::open(&db_path).expect("reopen");
        let result = index.get_package_coupling("broken");
        assert!(
            result.is_err(),
            "corrupt target source should return Err, got {result:?}"
        );
    }

    /// The documented contract for `fetch_neighbors` is "silent skip + warn! log" when
    /// a neighbour has a corrupt source value. This test verifies both halves: the
    /// silent-skip (corrupt neighbour absent from the outgoing list) and the
    /// observability (a warn! event mentioning the corrupt value was emitted).
    ///
    /// If someone removes the `warn!` call, the test will fail on the log assertion —
    /// catching a regression where the behaviour silently becomes "completely silent"
    /// rather than "silent + logged".
    #[traced_test]
    #[test]
    fn fetch_neighbors_skips_neighbors_with_corrupt_source() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("idx.db");

        // Set up two packages where the target is valid and one neighbour is corrupt.
        let (f_target, f_valid, f_bad) = {
            let mut index = Index::open(&db_path).expect("schema");
            let f_target = index
                .upsert_file(Path::new("target/lib.rs"), Language::Rust, 0, 0, None)
                .expect("target");
            let f_valid = index
                .upsert_file(Path::new("valid/lib.rs"), Language::Rust, 0, 0, None)
                .expect("valid");
            let f_bad = index
                .upsert_file(Path::new("bad/lib.rs"), Language::Rust, 0, 0, None)
                .expect("bad");
            // target → valid AND target → bad
            index
                .insert_file_dependency(f_target, f_valid)
                .expect("→valid");
            index.insert_file_dependency(f_target, f_bad).expect("→bad");
            (f_target, f_valid, f_bad)
        };

        // Insert the three packages via raw SQL so we can plant a corrupt source.
        let raw = Connection::open(&db_path).expect("raw open");
        raw.execute_batch(&format!(
            "PRAGMA ignore_check_constraints = ON;
             DELETE FROM arch_packages;
             INSERT INTO arch_packages (id, name, path, source)
             VALUES (1, 'target', 'target', 'manifest');
             INSERT INTO arch_packages (id, name, path, source)
             VALUES (2, 'valid', 'valid', 'manifest');
             INSERT INTO arch_packages (id, name, path, source)
             VALUES (3, 'bad', 'bad', 'totally-bogus');
             INSERT INTO arch_file_packages (file_id, package_id)
             VALUES ({target_id}, 1);
             INSERT INTO arch_file_packages (file_id, package_id)
             VALUES ({valid_id}, 2);
             INSERT INTO arch_file_packages (file_id, package_id)
             VALUES ({bad_id}, 3);
             INSERT INTO arch_package_deps (source_pkg, target_pkg, dep_count)
             VALUES (1, 2, 1);
             INSERT INTO arch_package_deps (source_pkg, target_pkg, dep_count)
             VALUES (1, 3, 1);",
            target_id = f_target.as_i64(),
            valid_id = f_valid.as_i64(),
            bad_id = f_bad.as_i64(),
        ))
        .expect("seed");
        drop(raw);

        let index = Index::open(&db_path).expect("reopen");
        let detail = index
            .get_package_coupling("target")
            .expect("target query should succeed (its source is valid)")
            .expect("target package exists");

        // Neighbour 'valid' should appear; 'bad' should be silently skipped.
        let neighbour_names: Vec<_> = detail
            .outgoing
            .iter()
            .map(|d| d.package.name.as_str())
            .collect();
        assert_eq!(
            neighbour_names,
            ["valid"],
            "corrupt-source neighbour should be skipped from outgoing list"
        );

        // CONTRACT: fetch_neighbors must emit a warn! when skipping a corrupt neighbour.
        // Removing that warn! would change the behaviour from "silent + logged" to
        // "completely silent", which breaks the observability guarantee. If this assertion
        // fails, restore the warn! call in fetch_neighbors rather than loosening the test.
        assert!(
            logs_contain("unknown source value"),
            "expected a warn! log mentioning the corrupt source value"
        );
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

        let f_a = index
            .upsert_file(Path::new("a/lib.rs"), Language::Rust, 0, 0, None)
            .expect("a");
        let f_b = index
            .upsert_file(Path::new("b/lib.rs"), Language::Rust, 0, 0, None)
            .expect("b");
        let f_c = index
            .upsert_file(Path::new("c/lib.rs"), Language::Rust, 0, 0, None)
            .expect("c");

        index.insert_file_dependency(f_a, f_b).expect("a→b");
        index.insert_file_dependency(f_a, f_c).expect("a→c");
        index.insert_file_dependency(f_b, f_c).expect("b→c");

        let packages = [
            PackageInsert {
                name: "a",
                path: "a",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "b",
                path: "b",
                source: PackageSource::Manifest,
            },
            PackageInsert {
                name: "c",
                path: "c",
                source: PackageSource::Manifest,
            },
        ];
        let mappings = [(f_a, "a"), (f_b, "b"), (f_c, "c")];
        index
            .repopulate_architecture(&packages, &mappings)
            .expect("repopulate");

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
        let rows = index
            .get_coupling_metrics(CouplingSort::Name)
            .expect("metrics");

        let a = metrics_for(&rows, "a");
        assert_eq!((a.afferent, a.efferent), (0, 2));
        assert!((a.instability() - 1.0).abs() < 1e-9);

        let b = metrics_for(&rows, "b");
        assert_eq!((b.afferent, b.efferent), (1, 1));
        assert!((b.instability() - 0.5).abs() < 1e-9);

        let c = metrics_for(&rows, "c");
        assert_eq!((c.afferent, c.efferent), (2, 0));
        assert!((c.instability() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn sort_by_instability_descending() {
        let (_dir, index) = seeded_index();
        let rows = index
            .get_coupling_metrics(CouplingSort::Instability)
            .expect("metrics");
        assert_eq!(rows[0].package.name, "a");
        assert_eq!(rows[1].package.name, "b");
        assert_eq!(rows[2].package.name, "c");
    }

    #[test]
    fn sort_by_name_ascending() {
        let (_dir, index) = seeded_index();
        let rows = index
            .get_coupling_metrics(CouplingSort::Name)
            .expect("metrics");
        assert_eq!(rows[0].package.name, "a");
        assert_eq!(rows[1].package.name, "b");
        assert_eq!(rows[2].package.name, "c");
    }

    #[test]
    fn isolated_package_has_zero_instability() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        let packages = [PackageInsert {
            name: "lonely",
            path: "lonely",
            source: PackageSource::Manifest,
        }];
        index
            .repopulate_architecture(&packages, &[])
            .expect("repopulate");

        let rows = index
            .get_coupling_metrics(CouplingSort::Name)
            .expect("metrics");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].afferent, rows[0].efferent), (0, 0));
        assert!(rows[0].instability().abs() < 1e-9);
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

#[cfg(test)]
mod instability_property_tests {
    use proptest::prelude::*;
    use rusqlite::Connection;

    /// Build an in-memory DB with `n` packages and the listed cross-package edges,
    /// then query `arch_coupling`. Edges are (`source_index`, `target_index`) pairs.
    ///
    /// Returns `(afferent, efferent, instability)` triples where `instability` is
    /// computed in Rust via the same formula as `CouplingMetrics::instability()` —
    /// not from the view, which no longer stores the computed column.
    fn instability_for(n: usize, edges: &[(usize, usize)]) -> Vec<(u32, u32, f64)> {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(crate::db::SCHEMA).expect("schema");

        for i in 0..n {
            conn.execute(
                "INSERT INTO arch_packages (id, name, path, source) VALUES (?1, ?2, ?3, 'manifest')",
                rusqlite::params![i64::try_from(i + 1).expect("package index fits in i64"), format!("p{i}"), format!("p{i}")],
            )
            .expect("insert pkg");
        }
        for (src, tgt) in edges {
            if src == tgt {
                continue;
            }
            // INSERT OR IGNORE to dedupe (src, tgt) pairs (PK constraint).
            conn.execute(
                "INSERT OR IGNORE INTO arch_package_deps (source_pkg, target_pkg, dep_count)
                 VALUES (?1, ?2, 1)",
                rusqlite::params![
                    i64::try_from(src + 1).expect("source index fits in i64"),
                    i64::try_from(tgt + 1).expect("target index fits in i64")
                ],
            )
            .expect("insert dep");
        }

        let mut stmt = conn
            .prepare("SELECT afferent, efferent FROM arch_coupling")
            .expect("prepare");
        let rows: Vec<(u32, u32, f64)> = stmt
            .query_map([], |r| {
                Ok((
                    u32::try_from(r.get::<_, i64>(0)?).unwrap_or(u32::MAX),
                    u32::try_from(r.get::<_, i64>(1)?).unwrap_or(u32::MAX),
                ))
            })
            .expect("query")
            .map(|r| {
                let (ca, ce) = r.expect("row");
                let instability = if ca + ce == 0 {
                    0.0
                } else {
                    f64::from(ce) / f64::from(ca + ce)
                };
                (ca, ce, instability)
            })
            .collect();
        rows
    }

    proptest! {
        /// For every package and every random edge set, instability stays in [0, 1].
        #[test]
        fn instability_within_unit_interval(
            n in 1usize..8,
            edges in prop::collection::vec((0usize..8, 0usize..8), 0..30),
        ) {
            let edges: Vec<_> = edges.into_iter()
                .filter(|(s, t)| *s < n && *t < n)
                .collect();
            for (_ca, _ce, i) in instability_for(n, &edges) {
                prop_assert!((0.0..=1.0).contains(&i), "instability out of range: {i}");
            }
        }

        /// A package with no edges has instability exactly 0.
        #[test]
        fn isolated_package_has_zero_instability(n in 1usize..6) {
            let rows = instability_for(n, &[]);
            for (ca, ce, i) in rows {
                prop_assert_eq!((ca, ce), (0, 0));
                prop_assert!((i - 0.0_f64).abs() < 1e-9);
            }
        }
    }
}
