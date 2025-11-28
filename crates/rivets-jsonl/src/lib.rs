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
//!
//! ## Resilient Streaming
//!
//! Process JSONL files that may contain malformed lines, collecting warnings
//! for problematic lines while continuing to process valid data:
//!
//! ```no_run
//! use rivets_jsonl::JsonlReader;
//! use futures::stream::StreamExt;
//! use serde::Deserialize;
//! use tokio::fs::File;
//!
//! #[derive(Deserialize)]
//! struct Record { id: u32, name: String }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("data.jsonl").await?;
//! let reader = JsonlReader::new(file);
//!
//! // Stream continues despite malformed JSON lines
//! let (stream, warnings) = reader.stream_resilient::<Record>();
//!
//! let records: Vec<Record> = stream.collect().await;
//!
//! // Check warnings after processing
//! for warning in warnings.warnings() {
//!     eprintln!("Warning: {}", warning);
//! }
//!
//! println!("Loaded {} valid records with {} warnings",
//!          records.len(), warnings.len());
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod atomic;
pub mod error;
pub mod query;
pub mod reader;
pub mod stream;
pub mod warning;
pub mod writer;

pub use atomic::{write_jsonl_atomic, write_jsonl_atomic_iter};
pub use error::{Error, Result};
pub use reader::JsonlReader;
pub use warning::{Warning, WarningCollector};
pub use writer::JsonlWriter;

use futures::stream::StreamExt;
use serde::de::DeserializeOwned;
use std::path::Path;
use tokio::fs::File;

/// Reads an entire JSONL file into a Vec while resiliently handling malformed lines.
///
/// This is a convenience function that combines file opening, resilient streaming,
/// and collection into a single operation. Malformed JSON lines are skipped and
/// their errors are collected as warnings rather than causing the operation to fail.
///
/// # Type Parameters
///
/// * `T` - The type to deserialize each JSON line into. Must implement [`DeserializeOwned`]
///   and have a `'static` lifetime bound.
/// * `P` - The path type, which can be anything that implements [`AsRef<Path>`].
///
/// # Returns
///
/// Returns `Ok((Vec<T>, Vec<Warning>))` containing:
/// - A vector of all successfully parsed records
/// - A vector of warnings for any malformed lines encountered
///
/// Returns `Err` if the file cannot be opened (e.g., file not found, permission denied).
///
/// # Warning Collection
///
/// Each malformed line generates a [`Warning::MalformedJson`] entry containing:
/// - The 1-based line number where the error occurred
/// - A description of the parsing error
///
/// Empty lines and whitespace-only lines are silently skipped without generating warnings.
///
/// # I/O Errors
///
/// I/O errors during reading (after the file is opened) will terminate the stream
/// early. The function will return all records successfully read up to that point
/// along with any warnings collected.
///
/// # Memory Usage
///
/// This function loads the entire file into memory at once. For large files,
/// consider the following:
///
/// - **Small to medium files** (< 100MB): Use `read_jsonl_resilient()` for simplicity
/// - **Large files** (> 100MB): Use [`JsonlReader::stream_resilient()`] for constant
///   memory usage with streaming processing
/// - **Memory-constrained environments**: Always prefer streaming over loading entire
///   files into memory
///
/// The memory footprint includes:
/// - All successfully parsed records in the returned `Vec<T>`
/// - All warnings in the returned `Vec<Warning>`
/// - Temporary buffers used during deserialization
///
/// # Examples
///
/// ## Basic Usage
///
/// ```no_run
/// use rivets_jsonl::read_jsonl_resilient;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Record {
///     id: u32,
///     name: String,
/// }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let (records, warnings) = read_jsonl_resilient::<Record, _>("data.jsonl").await?;
///
/// println!("Loaded {} records with {} warnings", records.len(), warnings.len());
///
/// for warning in &warnings {
///     eprintln!("Warning: {}", warning);
/// }
/// # Ok(())
/// # }
/// ```
///
/// ## Handling a Corrupted File
///
/// ```no_run
/// use rivets_jsonl::read_jsonl_resilient;
/// use serde::Deserialize;
/// use std::path::PathBuf;
///
/// #[derive(Deserialize)]
/// struct Issue {
///     id: String,
///     title: String,
///     status: String,
/// }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let path = PathBuf::from(".beads/issues.jsonl");
/// let (issues, warnings) = read_jsonl_resilient::<Issue, _>(&path).await?;
///
/// if !warnings.is_empty() {
///     eprintln!("Warning: {} lines could not be parsed", warnings.len());
///     for warning in &warnings {
///         eprintln!("  Line {}: {}", warning.line_number(), warning);
///     }
/// }
///
/// println!("Successfully loaded {} issues", issues.len());
/// # Ok(())
/// # }
/// ```
///
/// ## File Not Found Handling
///
/// ```no_run
/// use rivets_jsonl::read_jsonl_resilient;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Config {
///     key: String,
///     value: String,
/// }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// match read_jsonl_resilient::<Config, _>("nonexistent.jsonl").await {
///     Ok((configs, warnings)) => {
///         println!("Loaded {} configs", configs.len());
///     }
///     Err(e) => {
///         eprintln!("Failed to open file: {}", e);
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn read_jsonl_resilient<T, P>(path: P) -> Result<(Vec<T>, Vec<Warning>)>
where
    T: DeserializeOwned + 'static,
    P: AsRef<Path>,
{
    let file = File::open(path).await?;
    let reader = JsonlReader::new(file);
    let (stream, collector) = reader.stream_resilient();

    let values: Vec<T> = std::pin::pin!(stream).collect().await;
    let warnings = collector.into_warnings();

    Ok((values, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestRecord {
        id: u32,
        name: String,
    }

    /// Helper to create a temporary file with the given content.
    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write to temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    mod read_jsonl_resilient_tests {
        use super::*;

        #[tokio::test]
        async fn reads_empty_file() {
            let file = create_temp_file("");
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert!(records.is_empty());
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn reads_single_valid_record() {
            let file = create_temp_file(r#"{"id": 1, "name": "Alice"}"#);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 1);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[0].name, "Alice");
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn reads_multiple_valid_records() {
            let content = r#"{"id": 1, "name": "Alice"}
{"id": 2, "name": "Bob"}
{"id": 3, "name": "Charlie"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 3);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[0].name, "Alice");
            assert_eq!(records[1].id, 2);
            assert_eq!(records[1].name, "Bob");
            assert_eq!(records[2].id, 3);
            assert_eq!(records[2].name, "Charlie");
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn collects_warnings_for_malformed_lines() {
            let content = r#"{"id": 1, "name": "Alice"}
{invalid json}
{"id": 3, "name": "Charlie"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 2);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[1].id, 3);

            assert_eq!(warnings.len(), 1);
            assert_eq!(warnings[0].line_number(), 2);
        }

        #[tokio::test]
        async fn collects_warnings_for_type_mismatch() {
            let content = r#"{"id": 1, "name": "Alice"}
{"id": "not_a_number", "name": "Bob"}
{"id": 3, "name": "Charlie"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 2);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[1].id, 3);

            assert_eq!(warnings.len(), 1);
            assert_eq!(warnings[0].line_number(), 2);
        }

        #[tokio::test]
        async fn handles_multiple_malformed_lines() {
            let content = r#"{"id": 1, "name": "Alice"}
{invalid1}
{invalid2}
{"id": 4, "name": "Diana"}
{invalid3}
{"id": 6, "name": "Frank"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 3);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[1].id, 4);
            assert_eq!(records[2].id, 6);

            assert_eq!(warnings.len(), 3);
            assert_eq!(warnings[0].line_number(), 2);
            assert_eq!(warnings[1].line_number(), 3);
            assert_eq!(warnings[2].line_number(), 5);
        }

        #[tokio::test]
        async fn handles_all_malformed_lines() {
            let content = r#"{invalid1}
{invalid2}
{invalid3}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert!(records.is_empty());
            assert_eq!(warnings.len(), 3);
            assert_eq!(warnings[0].line_number(), 1);
            assert_eq!(warnings[1].line_number(), 2);
            assert_eq!(warnings[2].line_number(), 3);
        }

        #[tokio::test]
        async fn skips_empty_lines() {
            let content = r#"
{"id": 1, "name": "Alice"}

{"id": 2, "name": "Bob"}
"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 2);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[1].id, 2);
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn skips_whitespace_only_lines() {
            let content =
                "   \n{\"id\": 1, \"name\": \"Alice\"}\n\t\t\n{\"id\": 2, \"name\": \"Bob\"}\n";
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 2);
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn returns_error_for_nonexistent_file() {
            let result =
                read_jsonl_resilient::<TestRecord, _>("/nonexistent/path/file.jsonl").await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn handles_file_without_trailing_newline() {
            let content = r#"{"id": 1, "name": "Alice"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 1);
            assert_eq!(records[0].id, 1);
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn handles_whitespace_around_json() {
            let content = "  {\"id\": 1, \"name\": \"Alice\"}  \n";
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 1);
            assert_eq!(records[0].id, 1);
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn preserves_record_order() {
            let content = (0..100)
                .map(|i| format!(r#"{{"id": {}, "name": "Record{}"}}"#, i, i))
                .collect::<Vec<_>>()
                .join("\n");
            let file = create_temp_file(&content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 100);
            for (i, record) in records.iter().enumerate() {
                assert_eq!(record.id, i as u32);
            }
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn handles_unicode_content() {
            let content = r#"{"id": 1, "name": "Hello, 世界!"}
{"id": 2, "name": "Emoji: 😀🎉"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 2);
            assert_eq!(records[0].name, "Hello, 世界!");
            assert_eq!(records[1].name, "Emoji: 😀🎉");
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn handles_special_characters() {
            let content = r#"{"id": 1, "name": "Line1\nLine2\tTabbed"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 1);
            assert_eq!(records[0].name, "Line1\nLine2\tTabbed");
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn handles_large_file() {
            let content = (0..1000)
                .map(|i| format!(r#"{{"id": {}, "name": "Record{}"}}"#, i, i))
                .collect::<Vec<_>>()
                .join("\n");
            let file = create_temp_file(&content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 1000);
            assert!(warnings.is_empty());
        }

        #[tokio::test]
        async fn warning_contains_error_details() {
            let content = r#"{"id": "not_a_number", "name": "Invalid"}"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert!(records.is_empty());
            assert_eq!(warnings.len(), 1);

            match &warnings[0] {
                Warning::MalformedJson { line_number, error } => {
                    assert_eq!(*line_number, 1);
                    assert!(!error.is_empty());
                }
                _ => panic!("Expected MalformedJson warning"),
            }
        }

        #[tokio::test]
        async fn works_with_pathbuf() {
            let file = create_temp_file(r#"{"id": 1, "name": "Test"}"#);
            let path = std::path::PathBuf::from(file.path());
            let (records, _) = read_jsonl_resilient::<TestRecord, _>(&path).await.unwrap();
            assert_eq!(records.len(), 1);
        }

        #[tokio::test]
        async fn works_with_string_path() {
            let file = create_temp_file(r#"{"id": 1, "name": "Test"}"#);
            let path = file.path().to_str().unwrap().to_string();
            let (records, _) = read_jsonl_resilient::<TestRecord, _>(&path).await.unwrap();
            assert_eq!(records.len(), 1);
        }

        #[tokio::test]
        async fn mixed_valid_invalid_and_empty_lines() {
            let content = r#"
{"id": 1, "name": "Alice"}

{invalid json}

{"id": 2, "name": "Bob"}
{"missing_id": true}
{"id": 3, "name": "Charlie"}
"#;
            let file = create_temp_file(content);
            let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(file.path())
                .await
                .unwrap();

            assert_eq!(records.len(), 3);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[1].id, 2);
            assert_eq!(records[2].id, 3);

            // 2 warnings: invalid json line and missing required field line
            assert_eq!(warnings.len(), 2);
        }
    }
}
