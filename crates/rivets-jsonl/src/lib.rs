//! A high-performance JSONL (JSON Lines) library for Rust.
//!
//! This library provides efficient reading, writing, streaming, and querying
//! of JSONL (JSON Lines) formatted data.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod query;
pub mod reader;
pub mod stream;
pub mod writer;

pub use error::{Error, Result};
