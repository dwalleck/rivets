//! Rivets - A Rust-based issue tracking system.
//!
//! This crate provides both a CLI application and a library for issue tracking
//! using various storage backends.

#![forbid(unsafe_code)]

// Public modules for library usage
pub mod domain;
pub mod error;
pub mod id_generation;
pub mod storage;

// Public CLI module (needed by binary)
pub mod cli;

// Command implementations
pub mod commands;

// Internal modules (not exposed as public API)
pub(crate) mod config;
