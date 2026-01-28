//! Benchmarks for Tethys indexing operations.
//!
//! These benchmarks measure the performance of:
//! - Full workspace indexing
//! - Pass 1 (file extraction) vs Pass 2 (cross-file resolution) timing
//! - Scaling behavior with different workspace sizes

// Benchmark code - performance of the benchmark setup is not critical
#![allow(missing_docs)]
#![allow(clippy::format_push_string)]
#![allow(clippy::cast_possible_truncation)]

mod common;

use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tethys::Tethys;

use common::{as_file_refs, create_indexed_workspace, create_workspace};

/// Generate a realistic Rust file with multiple symbols and references.
fn generate_rust_file(module_name: &str, num_functions: usize, num_structs: usize) -> String {
    let mut code = String::new();

    code.push_str("use std::collections::HashMap;\n");
    code.push_str("use std::sync::Arc;\n\n");

    for i in 0..num_structs {
        code.push_str(&format!(
            "pub struct {module_name}Struct{i} {{\n\
                 pub id: i64,\n\
                 pub name: String,\n\
                 pub data: HashMap<String, i64>,\n\
             }}\n\n\
             impl {module_name}Struct{i} {{\n\
                 pub fn new(id: i64, name: String) -> Self {{\n\
                     Self {{ id, name, data: HashMap::new() }}\n\
                 }}\n\n\
                 pub fn process(&mut self) -> i64 {{\n\
                     self.data.values().sum()\n\
                 }}\n\
             }}\n\n"
        ));
    }

    for i in 0..num_functions {
        code.push_str(&format!(
            "pub fn {module_name}_func{i}(input: i64) -> i64 {{\n\
                 let result = input * 2;\n\
                 result + {i}\n\
             }}\n\n"
        ));
    }

    code.push_str(&format!(
        "pub fn {module_name}_main() -> i64 {{\n\
             let mut total = 0i64;\n"
    ));

    for i in 0..num_functions.min(5) {
        code.push_str(&format!("    total += {module_name}_func{i}(total);\n"));
    }

    for i in 0..num_structs.min(3) {
        code.push_str(&format!(
            "    let mut s{i} = {module_name}Struct{i}::new({i}, \"test\".to_string());\n\
                 total += s{i}.process();\n"
        ));
    }

    code.push_str("    total\n}\n");

    code
}

/// Generate a workspace with multiple modules for cross-file reference testing.
fn generate_multi_module_workspace(num_modules: usize) -> Vec<(String, String)> {
    let mut files = Vec::new();

    // Create lib.rs with mod declarations and re-exports
    let mut lib_content = String::new();
    for i in 0..num_modules {
        lib_content.push_str(&format!("mod module{i};\n"));
    }
    lib_content.push('\n');
    for i in 0..num_modules {
        lib_content.push_str(&format!("pub use module{i}::*;\n"));
    }
    files.push(("src/lib.rs".to_string(), lib_content));

    // Create individual module files
    for i in 0..num_modules {
        let module_name = format!("module{i}");
        let mut content = String::new();

        if i > 0 {
            content.push_str(&format!("use crate::module{}::*;\n\n", i - 1));
        }

        content.push_str(&generate_rust_file(&module_name, 5, 3));

        if i > 0 {
            content.push_str(&format!(
                "\npub fn cross_module_call{i}() -> i64 {{\n\
                     module{}_main()\n\
                 }}\n",
                i - 1
            ));
        }

        files.push((format!("src/module{i}.rs"), content));
    }

    files
}

/// Benchmark full index operation on workspaces of different sizes.
fn bench_full_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_index");

    for num_modules in &[1, 5, 10, 20] {
        let files = generate_multi_module_workspace(*num_modules);
        let file_refs = as_file_refs(&files);

        group.throughput(Throughput::Elements(*num_modules as u64));

        group.bench_with_input(
            BenchmarkId::new("modules", num_modules),
            num_modules,
            |b, _| {
                b.iter_with_setup(
                    || {
                        let (dir, path) = create_workspace(&file_refs);
                        let tethys = Tethys::new(&path).expect("failed to create Tethys");
                        (dir, tethys)
                    },
                    |(_dir, mut tethys)| {
                        let stats = tethys.index().expect("index failed");
                        black_box(stats)
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark to measure timing breakdown of indexing phases.
///
/// This doesn't use criterion's measurement but provides visibility into phase timing.
fn bench_indexing_phases(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexing_phases");

    let files = generate_multi_module_workspace(10);
    let file_refs = as_file_refs(&files);

    group.bench_function("total_index_time", |b| {
        b.iter_with_setup(
            || {
                let (dir, path) = create_workspace(&file_refs);
                let tethys = Tethys::new(&path).expect("failed to create Tethys");
                (dir, tethys)
            },
            |(_dir, mut tethys)| {
                let stats = tethys.index().expect("index failed");
                black_box(stats)
            },
        );
    });

    group.finish();

    // Print detailed timing for a single run (outside criterion measurement)
    println!("\n=== Detailed Indexing Phase Timing ===");

    let (dir, path) = create_workspace(&file_refs);
    let mut tethys = Tethys::new(&path).expect("failed to create Tethys");

    let start = Instant::now();
    let stats = tethys.index().expect("index failed");
    let total_duration = start.elapsed();

    println!("Files indexed: {}", stats.files_indexed);
    println!("Symbols found: {}", stats.symbols_found);
    println!("References found: {}", stats.references_found);
    println!("Total duration: {total_duration:?}");
    println!(
        "Avg per file: {:?}",
        total_duration / stats.files_indexed.max(1) as u32
    );
    println!(
        "Avg per symbol: {:?}",
        total_duration / stats.symbols_found.max(1) as u32
    );

    drop(dir);
}

/// Benchmark re-indexing (update scenario).
fn bench_reindex(c: &mut Criterion) {
    let mut group = c.benchmark_group("reindex");

    let files = generate_multi_module_workspace(10);
    let file_refs = as_file_refs(&files);

    group.bench_function("reindex_unchanged", |b| {
        b.iter_with_setup(
            || create_indexed_workspace(&file_refs),
            |mut workspace| {
                let stats = workspace.tethys.index().expect("reindex failed");
                black_box(stats)
            },
        );
    });

    group.finish();
}

/// Benchmark database operations after indexing.
fn bench_post_index_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("post_index");

    let files = generate_multi_module_workspace(10);
    let file_refs = as_file_refs(&files);
    let workspace = create_indexed_workspace(&file_refs);
    let tethys = workspace.tethys;

    group.bench_function("search_symbols", |b| {
        b.iter(|| {
            let results = tethys.search_symbols("module").expect("search failed");
            black_box(results)
        });
    });

    group.bench_function("get_stats", |b| {
        b.iter(|| {
            let stats = tethys.get_stats().expect("get_stats failed");
            black_box(stats)
        });
    });

    let file_path = workspace.dir.path().join("src/module5.rs");
    group.bench_function("list_symbols_in_file", |b| {
        b.iter(|| {
            let symbols = tethys
                .list_symbols(&file_path)
                .expect("list_symbols failed");
            black_box(symbols)
        });
    });

    group.finish();

    drop(workspace.dir);
}

/// Benchmark scaling behavior with number of symbols per file.
fn bench_symbol_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_density");

    for (num_functions, num_structs) in &[(5, 2), (20, 10), (50, 25)] {
        let file_content = generate_rust_file("dense", *num_functions, *num_structs);
        let files = vec![("src/lib.rs", file_content.as_str())];

        let total_expected_symbols = num_functions + num_structs * 3;

        group.throughput(Throughput::Elements(total_expected_symbols as u64));

        group.bench_with_input(
            BenchmarkId::new("symbols", total_expected_symbols),
            &files,
            |b, files| {
                b.iter_with_setup(
                    || {
                        let (dir, path) = create_workspace(files);
                        let tethys = Tethys::new(&path).expect("failed to create Tethys");
                        (dir, tethys)
                    },
                    |(_dir, mut tethys)| {
                        let stats = tethys.index().expect("index failed");
                        black_box(stats)
                    },
                );
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_full_index,
    bench_indexing_phases,
    bench_reindex,
    bench_post_index_operations,
    bench_symbol_density,
);

criterion_main!(benches);
