//! Benchmarks for Tethys query operations.
//!
//! These benchmarks measure the performance of:
//! - `get_callers` with varying numbers of callers
//! - `get_symbol_impact` for transitive caller analysis
//! - Database index effectiveness

// Benchmark code - performance of the benchmark setup is not critical
#![allow(missing_docs)]
#![allow(clippy::format_push_string)]

mod common;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use common::{as_file_refs, create_indexed_workspace};

/// Generate a workspace with a target function and N caller functions.
///
/// Creates a structure like:
/// ```text
/// target_func (called by)
///   <- caller_1
///   <- caller_2
///   <- ...
///   <- caller_N
/// ```
fn generate_caller_test_workspace(num_callers: usize) -> Vec<(String, String)> {
    let mut files = Vec::new();

    // lib.rs with mod declarations
    let mut lib_content = String::from("mod target;\n");
    for i in 0..num_callers {
        lib_content.push_str(&format!("mod caller{i};\n"));
    }
    files.push(("src/lib.rs".to_string(), lib_content));

    // target.rs - the function that gets called
    let target_content = "\
pub fn target_func(x: i64) -> i64 {
    x * 2
}

pub struct TargetStruct;

impl TargetStruct {
    pub fn process(&self, x: i64) -> i64 {
        x + 1
    }
}
";
    files.push(("src/target.rs".to_string(), target_content.to_string()));

    // Generate caller files
    for i in 0..num_callers {
        let caller_content = format!(
            "use crate::target::{{target_func, TargetStruct}};\n\n\
             pub fn caller{i}_a() -> i64 {{\n\
                 target_func({i})\n\
             }}\n\n\
             pub fn caller{i}_b() -> i64 {{\n\
                 let s = TargetStruct;\n\
                 s.process(target_func({i}))\n\
             }}\n\n\
             pub fn caller{i}_c() -> i64 {{\n\
                 // Multiple calls to target_func\n\
                 target_func(1) + target_func(2) + target_func(3)\n\
             }}\n"
        );
        files.push((format!("src/caller{i}.rs"), caller_content));
    }

    files
}

/// Generate a deep call chain for transitive caller testing.
///
/// Creates a structure like:
/// ```text
/// depth_0_func (called by)
///   <- depth_1_func (called by)
///     <- depth_2_func (called by)
///       <- ... (called by)
///         <- depth_N_func
/// ```
fn generate_deep_call_chain(depth: usize) -> Vec<(String, String)> {
    let mut files = Vec::new();

    // lib.rs with mod declarations
    let mut lib_content = String::new();
    for i in 0..=depth {
        lib_content.push_str(&format!("mod depth{i};\n"));
    }
    files.push(("src/lib.rs".to_string(), lib_content));

    // Generate depth files
    for i in 0..=depth {
        let content = if i == 0 {
            "pub fn depth0_func(x: i64) -> i64 {\n\
                 x * 2\n\
             }\n"
            .to_string()
        } else {
            format!(
                "use crate::depth{}::depth{}_func;\n\n\
                 pub fn depth{i}_func(x: i64) -> i64 {{\n\
                     depth{}_func(x) + {i}\n\
                 }}\n",
                i - 1,
                i - 1,
                i - 1
            )
        };
        files.push((format!("src/depth{i}.rs"), content));
    }

    files
}

/// Generate a workspace with both wide (many callers) and deep (call chains) patterns.
fn generate_mixed_call_graph(width: usize) -> Vec<(String, String)> {
    let mut files = Vec::new();

    // lib.rs
    let mut lib_content = String::from("mod core;\nmod layer1;\nmod layer2;\n");
    for i in 0..width {
        lib_content.push_str(&format!("mod consumer{i};\n"));
    }
    files.push(("src/lib.rs".to_string(), lib_content));

    // core.rs - base functions
    let core_content = "\
pub fn core_compute(x: i64) -> i64 {
    x * 2
}

pub fn core_transform(x: i64) -> i64 {
    x + 100
}
";
    files.push(("src/core.rs".to_string(), core_content.to_string()));

    // layer1.rs - calls core
    let layer1_content = "\
use crate::core::{core_compute, core_transform};

pub fn layer1_process(x: i64) -> i64 {
    core_compute(x) + core_transform(x)
}
";
    files.push(("src/layer1.rs".to_string(), layer1_content.to_string()));

    // layer2.rs - calls layer1
    let layer2_content = "\
use crate::layer1::layer1_process;

pub fn layer2_aggregate(x: i64) -> i64 {
    layer1_process(x) + layer1_process(x + 1)
}
";
    files.push(("src/layer2.rs".to_string(), layer2_content.to_string()));

    // Generate consumer files (wide pattern)
    for i in 0..width {
        let consumer_content = format!(
            "use crate::layer2::layer2_aggregate;\n\
             use crate::layer1::layer1_process;\n\
             use crate::core::core_compute;\n\n\
             pub fn consumer{i}_main() -> i64 {{\n\
                 layer2_aggregate({i}) + layer1_process({i}) + core_compute({i})\n\
             }}\n"
        );
        files.push((format!("src/consumer{i}.rs"), consumer_content));
    }

    files
}

/// Benchmark `get_callers` with varying numbers of direct callers.
fn bench_get_callers(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_callers");

    for num_callers in &[1, 5, 10, 25, 50] {
        let files = generate_caller_test_workspace(*num_callers);
        let file_refs = as_file_refs(&files);
        let workspace = create_indexed_workspace(&file_refs);

        group.throughput(Throughput::Elements(*num_callers as u64));

        group.bench_with_input(
            BenchmarkId::new("callers", num_callers),
            num_callers,
            |b, _| {
                b.iter(|| {
                    let callers = workspace
                        .tethys
                        .get_callers("target_func")
                        .expect("get_callers failed");
                    black_box(callers)
                });
            },
        );

        drop(workspace.dir);
    }

    group.finish();
}

/// Benchmark `get_symbol_impact` with varying call chain depths.
fn bench_get_symbol_impact_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_impact_depth");

    for depth in &[2, 5, 10, 20] {
        let files = generate_deep_call_chain(*depth);
        let file_refs = as_file_refs(&files);
        let workspace = create_indexed_workspace(&file_refs);

        group.throughput(Throughput::Elements(*depth as u64));

        group.bench_with_input(BenchmarkId::new("depth", depth), depth, |b, _| {
            b.iter(|| {
                let impact = workspace
                    .tethys
                    .get_symbol_impact("depth0_func", None)
                    .expect("get_symbol_impact failed");
                black_box(impact)
            });
        });

        drop(workspace.dir);
    }

    group.finish();
}

/// Benchmark `get_symbol_impact` with mixed wide and deep patterns.
fn bench_get_symbol_impact_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_impact_mixed");

    for (width, depth) in &[(5, 3), (10, 3), (20, 3)] {
        let files = generate_mixed_call_graph(*width);
        let file_refs = as_file_refs(&files);
        let workspace = create_indexed_workspace(&file_refs);

        let label = format!("{width}w_{depth}d");
        group.throughput(Throughput::Elements((width * depth) as u64));

        group.bench_with_input(BenchmarkId::new("mixed", &label), &label, |b, _| {
            b.iter(|| {
                let impact = workspace
                    .tethys
                    .get_symbol_impact("core_compute", None)
                    .expect("get_symbol_impact failed");
                black_box(impact)
            });
        });

        drop(workspace.dir);
    }

    group.finish();
}

/// Benchmark file-level impact queries.
fn bench_get_file_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_impact");

    for num_callers in &[5, 10, 25] {
        let files = generate_caller_test_workspace(*num_callers);
        let file_refs = as_file_refs(&files);
        let workspace = create_indexed_workspace(&file_refs);
        let target_file = workspace.dir.path().join("src/target.rs");

        group.throughput(Throughput::Elements(*num_callers as u64));

        group.bench_with_input(
            BenchmarkId::new("dependents", num_callers),
            num_callers,
            |b, _| {
                b.iter(|| {
                    let impact = workspace
                        .tethys
                        .get_impact(&target_file, None)
                        .expect("get_impact failed");
                    black_box(impact)
                });
            },
        );

        drop(workspace.dir);
    }

    group.finish();
}

/// Benchmark `get_references` for symbols with varying reference counts.
fn bench_get_references(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_references");

    for num_callers in &[5, 10, 25] {
        let files = generate_caller_test_workspace(*num_callers);
        let file_refs = as_file_refs(&files);
        let workspace = create_indexed_workspace(&file_refs);

        // Each caller file has multiple calls to target_func
        let expected_refs = num_callers * 5; // 5 calls per caller (1 in a, 1 in b, 3 in c)

        group.throughput(Throughput::Elements(expected_refs as u64));

        group.bench_with_input(
            BenchmarkId::new("references", expected_refs),
            &expected_refs,
            |b, _| {
                b.iter(|| {
                    let refs = workspace
                        .tethys
                        .get_references("target_func")
                        .expect("get_references failed");
                    black_box(refs)
                });
            },
        );

        drop(workspace.dir);
    }

    group.finish();
}

/// Benchmark dependency chain finding.
fn bench_dependency_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("dependency_chain");

    for depth in &[3, 5, 10] {
        let files = generate_deep_call_chain(*depth);
        let file_refs = as_file_refs(&files);
        let workspace = create_indexed_workspace(&file_refs);

        let from_file = workspace.dir.path().join(format!("src/depth{depth}.rs"));
        let to_file = workspace.dir.path().join("src/depth0.rs");

        group.throughput(Throughput::Elements(*depth as u64));

        group.bench_with_input(BenchmarkId::new("chain_length", depth), depth, |b, _| {
            b.iter(|| {
                let chain = workspace
                    .tethys
                    .get_dependency_chain(&from_file, &to_file)
                    .expect("get_dependency_chain failed");
                black_box(chain)
            });
        });

        drop(workspace.dir);
    }

    group.finish();
}

/// Print database index usage analysis (not a benchmark, but useful for optimization).
fn analyze_query_plans(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_analysis");

    let files = generate_caller_test_workspace(20);
    let file_refs = as_file_refs(&files);
    let workspace = create_indexed_workspace(&file_refs);

    group.bench_function("baseline_query", |b| {
        b.iter(|| {
            let callers = workspace
                .tethys
                .get_callers("target_func")
                .expect("get_callers failed");
            black_box(callers)
        });
    });

    group.finish();

    // Print stats for analysis
    println!("\n=== Query Performance Analysis ===");
    let stats = workspace.tethys.get_stats().expect("get_stats failed");
    println!("Database stats:");
    println!("  Files: {}", stats.file_count);
    println!("  Symbols: {}", stats.symbol_count);
    println!("  References: {}", stats.reference_count);
    println!("  File dependencies: {}", stats.file_dependency_count);

    drop(workspace.dir);
}

criterion_group!(
    benches,
    bench_get_callers,
    bench_get_symbol_impact_depth,
    bench_get_symbol_impact_mixed,
    bench_get_file_impact,
    bench_get_references,
    bench_dependency_chain,
    analyze_query_plans,
);

criterion_main!(benches);
