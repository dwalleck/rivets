//! Petgraph operations for dependency analysis.
//!
//! This module provides graph algorithms for impact analysis, cycle detection,
//! and dependency path finding. It loads subgraphs from `SQLite` on-demand rather
//! than keeping the entire graph in memory.
//!
//! ## Design
//!
//! - `SQLite` is the source of truth
//! - Petgraph is used for algorithms that SQL can't express efficiently
//! - Subgraphs are loaded on-demand for specific operations
//!
//! ## Operations
//!
//! | Operation | Algorithm |
//! |-----------|-----------|
//! | Impact analysis | BFS from target node |
//! | Cycle detection | Tarjan's SCC algorithm |
//! | Shortest path | Dijkstra's algorithm |

// TODO: Phase 3 implementation
// - DependencyGraph struct wrapping petgraph::DiGraph
// - Load subgraph from SQLite
// - get_impact() using BFS
// - detect_cycles() using tarjan_scc
// - get_dependency_chain() using dijkstra
