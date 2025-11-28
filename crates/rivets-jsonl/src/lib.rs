//! A high-performance JSONL (JSON Lines) library for Rust.
//!
//! This library provides efficient async reading, writing, streaming, and querying
//! of JSONL (JSON Lines) formatted data.
//!
//! # Overview
//!
//! JSONL (JSON Lines) is a text format where each line is a valid JSON value.
//! This format is ideal for streaming data, log files, and large datasets that
//! don't fit in memory.
//!
//! # Core Types
//!
//! - [`JsonlReader`] - Async buffered reader for JSONL data with line tracking
//! - [`JsonlWriter`] - Async buffered writer for JSONL data
//!
//! # Examples
//!
//! ```no_run
//! use rivets_jsonl::{JsonlReader, JsonlWriter};
//! use tokio::fs::File;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Reading JSONL data
//! let file = File::open("data.jsonl").await?;
//! let reader = JsonlReader::new(file);
//!
//! // Writing JSONL data
//! let file = File::create("output.jsonl").await?;
//! let writer = JsonlWriter::new(file);
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod query;
pub mod reader;
pub mod stream;
pub mod writer;

pub use error::{Error, Result};
pub use reader::JsonlReader;
pub use writer::JsonlWriter;
