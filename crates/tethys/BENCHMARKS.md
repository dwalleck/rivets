# Tethys Performance Benchmarks

This document summarizes benchmark results for tethys indexing and query operations.

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench --manifest-path crates/tethys/Cargo.toml

# Run specific benchmark suite
cargo bench --manifest-path crates/tethys/Cargo.toml --bench indexing
cargo bench --manifest-path crates/tethys/Cargo.toml --bench queries
```

## Benchmark Results Summary

### Indexing Performance

| Workspace Size | Modules | Symbols | References | Time (ms) | Throughput |
|----------------|---------|---------|------------|-----------|------------|
| Small          | 1       | ~15     | ~40        | 32        | ~30 elem/s |
| Medium         | 5       | ~75     | ~200       | 168       | ~30 elem/s |
| Large          | 10      | ~150    | ~400       | 333       | ~30 elem/s |
| Very Large     | 20      | ~300    | ~800       | 670       | ~30 elem/s |

Key observations:
- Linear scaling with workspace size (good)
- ~30-32ms per module indexed
- Average ~2ms per symbol
- Re-indexing unchanged workspace takes similar time (no incremental optimization yet)

### Query Performance

| Query Type | Input Size | Time (us) | Throughput |
|------------|------------|-----------|------------|
| `get_callers` | 1 caller | 41 | 24K elem/s |
| `get_callers` | 5 callers | 115 | 44K elem/s |
| `get_callers` | 10 callers | 208 | 48K elem/s |
| `get_callers` | 25 callers | 502 | 50K elem/s |
| `get_callers` | 50 callers | 1006 | 50K elem/s |

| Query Type | Call Chain Depth | Time (us) | Throughput |
|------------|------------------|-----------|------------|
| `get_symbol_impact` | 2 levels | 73 | 27K elem/s |
| `get_symbol_impact` | 5 levels | 91 | 55K elem/s |
| `get_symbol_impact` | 10 levels | 122 | 82K elem/s |
| `get_symbol_impact` | 20 levels | 186 | 108K elem/s |

| Query Type | Width x Depth | Time (us) | Throughput |
|------------|---------------|-----------|------------|
| `get_symbol_impact` (mixed) | 5w x 3d | 109 | 138K elem/s |
| `get_symbol_impact` (mixed) | 10w x 3d | 144 | 208K elem/s |
| `get_symbol_impact` (mixed) | 20w x 3d | 224 | 268K elem/s |

| Query Type | References | Time (us) | Throughput |
|------------|------------|-----------|------------|
| `get_references` | 20 refs | 26 | 766K elem/s |
| `get_references` | 40 refs | 39 | 1M elem/s |
| `get_references` | 100 refs | 80 | 1.25M elem/s |

### Post-Index Operations

| Operation | Time (us) |
|-----------|-----------|
| `search_symbols` | 159 |
| `get_stats` | 20 |
| `list_symbols_in_file` | 24 |

## Performance Characteristics

### Scaling Behavior

1. **Indexing**: Linear O(n) with file count
2. **get_callers**: Linear O(k) with number of callers
3. **get_symbol_impact**: Sub-linear with depth due to BFS pruning
4. **get_references**: Excellent scaling with reference count

### Database Index Effectiveness

SQLite indexes are being used effectively:
- Symbol lookups by name: <1ms
- Reference queries: <100us typical
- File dependency queries: <50us

### Memory Considerations

- SQLite in-memory caching provides efficient repeated queries
- Tree-sitter parsing is the primary memory consumer during indexing
- Database file size scales linearly with codebase size

## Optimization Opportunities

### Identified

1. **Re-indexing**: Currently re-indexes all files even when unchanged
   - Potential: Implement mtime-based change detection
   - Impact: Could reduce re-index time by 90%+ for unchanged files

2. **Parallel Parsing**: Tree-sitter parsing is single-threaded
   - Potential: Parallel file parsing with rayon
   - Impact: Could improve indexing throughput 2-4x on multi-core

3. **Batch Database Operations**: Currently one transaction per file
   - Potential: Batch multiple files per transaction
   - Impact: ~20% improvement for large workspaces

### Database Index Verification

Current indexes appear effective based on query performance. The following indexes exist:
- `symbols(name)` - for symbol search
- `symbols(qualified_name)` - for exact lookups
- `references(symbol_id)` - for `get_references`
- `file_deps(file_id, depends_on_file_id)` - for dependency queries

## Real-World Reference

On the rivets codebase (~79 files, 1229 symbols, 15945 references):
- Full index time: ~14 seconds
- Average: ~177ms per file (including cross-file resolution)
- This is higher than synthetic benchmarks due to more complex cross-file dependencies

## Profiling Commands

For deeper analysis:

```bash
# Memory profiling with heaptrack
heaptrack cargo test --manifest-path crates/tethys/Cargo.toml index_multiple_files_in_workspace

# CPU profiling with perf
perf record cargo bench --manifest-path crates/tethys/Cargo.toml --bench indexing
perf report

# Flamegraph
cargo flamegraph --bench indexing -p tethys
```
