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
//!
//! # Streaming Patterns
//!
//! ## Processing Large Files with Constant Memory
//!
//! ```no_run
//! use rivets_jsonl::JsonlReader;
//! use futures::stream::StreamExt;
//! use serde::Deserialize;
//! use std::pin::pin;
//! use tokio::fs::File;
//!
//! #[derive(Deserialize)]
//! struct Record { id: u32, name: String }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("large.jsonl").await?;
//! let reader = JsonlReader::new(file);
//! let mut stream = pin!(reader.stream::<Record>());
//!
//! // Process one record at a time - constant memory usage
//! while let Some(result) = stream.next().await {
//!     let record = result?;
//!     // Process record...
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Filtering and Transforming
//!
//! ```no_run
//! use rivets_jsonl::JsonlReader;
//! use futures::stream::StreamExt;
//! use serde::Deserialize;
//! use tokio::fs::File;
//!
//! #[derive(Deserialize)]
//! struct Record { id: u32, active: bool, name: String }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("data.jsonl").await?;
//! let reader = JsonlReader::new(file);
//!
//! // Filter and collect valid, active records
//! let active_records: Vec<Record> = reader
//!     .stream::<Record>()
//!     .filter_map(|r| async move { r.ok() })
//!     .filter(|r| {
//!         let is_active = r.active;
//!         async move { is_active }
//!     })
//!     .collect()
//!     .await;
//! # Ok(())
//! # }
//! ```
//!
//! ## Taking a Subset
//!
//! ```no_run
//! use rivets_jsonl::JsonlReader;
//! use futures::stream::StreamExt;
//! use serde::Deserialize;
//! use tokio::fs::File;
//!
//! #[derive(Deserialize)]
//! struct Record { id: u32 }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("data.jsonl").await?;
//! let reader = JsonlReader::new(file);
//!
//! // Read only the first 100 records
//! let first_100: Vec<Record> = reader
//!     .stream::<Record>()
//!     .take(100)
//!     .filter_map(|r| async move { r.ok() })
//!     .collect()
//!     .await;
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
