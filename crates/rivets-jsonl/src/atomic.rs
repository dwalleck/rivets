//! Atomic write operations for JSONL files.
//!
//! This module provides functionality for atomically writing JSONL data to files,
//! ensuring crash safety by using the temp-file-then-rename pattern.
//!
//! # Atomicity Guarantee
//!
//! On POSIX systems, file renames within the same filesystem are atomic operations.
//! This module exploits this property to provide crash-safe writes:
//!
//! 1. Data is first written to a temporary file with a `.tmp` extension
//! 2. The temporary file is flushed and closed
//! 3. The temporary file is atomically renamed to the target path
//!
//! If a crash occurs during step 1 or 2, the original file remains intact.
//! The temporary file may be left behind, but data integrity is preserved.
//!
//! # Examples
//!
//! ```no_run
//! use rivets_jsonl::write_jsonl_atomic;
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! struct Record {
//!     id: u32,
//!     name: String,
//! }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let records = vec![
//!     Record { id: 1, name: "Alice".to_string() },
//!     Record { id: 2, name: "Bob".to_string() },
//! ];
//!
//! // Atomically write records to file
//! write_jsonl_atomic("data.jsonl", &records).await?;
//! # Ok(())
//! # }
//! ```

use crate::{JsonlWriter, Result};
use serde::Serialize;
use std::path::Path;
use tokio::fs::File;

/// Atomically writes a slice of values to a JSONL file.
///
/// This function provides crash-safe writing by first writing all data to a
/// temporary file, then atomically renaming it to the target path. This ensures
/// that the target file is never left in a partially-written state.
///
/// # Arguments
///
/// * `path` - The target file path. A temporary file with `.tmp` extension will
///   be created alongside it during the write operation.
/// * `values` - A slice of values to serialize and write. Each value is written
///   as a separate JSON line.
///
/// # Errors
///
/// Returns an error if:
/// - The temporary file cannot be created
/// - Any value fails to serialize
/// - An I/O error occurs during writing
/// - The atomic rename fails (e.g., cross-filesystem move)
///
/// # Atomicity
///
/// On failure, the original file (if it exists) is left unchanged. The temporary
/// file may be left behind and should be cleaned up by the caller or a subsequent
/// successful write.
///
/// # Examples
///
/// ```no_run
/// use rivets_jsonl::write_jsonl_atomic;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Issue {
///     id: String,
///     title: String,
/// }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let issues = vec![
///     Issue { id: "PROJ-1".to_string(), title: "First issue".to_string() },
///     Issue { id: "PROJ-2".to_string(), title: "Second issue".to_string() },
/// ];
///
/// // This write is atomic - either all issues are written or none
/// write_jsonl_atomic("issues.jsonl", &issues).await?;
/// # Ok(())
/// # }
/// ```
pub async fn write_jsonl_atomic<T, P>(path: P, values: &[T]) -> Result<()>
where
    T: Serialize,
    P: AsRef<Path>,
{
    write_jsonl_atomic_iter(path, values.iter()).await
}

/// Atomically writes an iterator of values to a JSONL file.
///
/// This is a more flexible version of [`write_jsonl_atomic`] that accepts any
/// iterator of serializable values. Useful when you want to avoid collecting
/// values into a slice first.
///
/// # Arguments
///
/// * `path` - The target file path.
/// * `values` - An iterator of values to serialize and write.
///
/// # Errors
///
/// See [`write_jsonl_atomic`] for error conditions.
///
/// # Examples
///
/// ```no_run
/// use rivets_jsonl::write_jsonl_atomic_iter;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Record { id: u32 }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Write records without collecting into a Vec first
/// let records = (0..1000).map(|id| Record { id });
/// write_jsonl_atomic_iter("records.jsonl", records).await?;
/// # Ok(())
/// # }
/// ```
pub async fn write_jsonl_atomic_iter<T, I, P>(path: P, values: I) -> Result<()>
where
    T: Serialize,
    I: IntoIterator<Item = T>,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let temp_path = make_temp_path(path);

    // Write to temporary file
    let write_result = write_to_temp_file(&temp_path, values).await;

    // Handle write failure: attempt to clean up temp file
    if let Err(e) = write_result {
        // Best-effort cleanup of temp file
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(e);
    }

    // Atomic rename to target path
    tokio::fs::rename(&temp_path, path).await?;

    Ok(())
}

/// Creates a temporary file path for atomic write operations.
///
/// The temp path is created by appending `.tmp` to the original filename.
/// If the original path has no extension, `.tmp` is appended directly.
/// If it has an extension, the extension is replaced with `{ext}.tmp`.
fn make_temp_path(path: &Path) -> std::path::PathBuf {
    let mut temp_path = path.to_path_buf();
    let new_extension = match path.extension() {
        Some(ext) => {
            let mut new_ext = ext.to_os_string();
            new_ext.push(".tmp");
            new_ext
        }
        None => std::ffi::OsString::from("tmp"),
    };
    temp_path.set_extension(new_extension);
    temp_path
}

/// Writes values to a temporary file, ensuring proper flush and close.
async fn write_to_temp_file<T, I>(temp_path: &Path, values: I) -> Result<()>
where
    T: Serialize,
    I: IntoIterator<Item = T>,
{
    let file = File::create(temp_path).await?;
    let mut writer = JsonlWriter::new(file);
    writer.write_all(values).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tokio::io::AsyncReadExt;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestRecord {
        id: u32,
        name: String,
    }

    #[test]
    fn make_temp_path_with_extension() {
        let path = Path::new("/path/to/file.jsonl");
        let temp = make_temp_path(path);
        assert_eq!(temp, Path::new("/path/to/file.jsonl.tmp"));
    }

    #[test]
    fn make_temp_path_without_extension() {
        let path = Path::new("/path/to/file");
        let temp = make_temp_path(path);
        assert_eq!(temp, Path::new("/path/to/file.tmp"));
    }

    #[test]
    fn make_temp_path_with_multiple_extensions() {
        let path = Path::new("/path/to/file.tar.gz");
        let temp = make_temp_path(path);
        assert_eq!(temp, Path::new("/path/to/file.tar.gz.tmp"));
    }

    #[test]
    fn make_temp_path_relative() {
        let path = Path::new("data.jsonl");
        let temp = make_temp_path(path);
        assert_eq!(temp, Path::new("data.jsonl.tmp"));
    }

    #[tokio::test]
    async fn write_to_temp_file_creates_valid_jsonl() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_write_temp.jsonl.tmp");

        let records = [
            TestRecord {
                id: 1,
                name: "Alice".to_string(),
            },
            TestRecord {
                id: 2,
                name: "Bob".to_string(),
            },
        ];

        write_to_temp_file(&temp_file, records.iter())
            .await
            .unwrap();

        // Verify file contents
        let mut file = File::open(&temp_file).await.unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).await.unwrap();

        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], r#"{"id":1,"name":"Alice"}"#);
        assert_eq!(lines[1], r#"{"id":2,"name":"Bob"}"#);

        // Cleanup
        tokio::fs::remove_file(&temp_file).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_creates_file() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_atomic_create.jsonl");

        // Ensure file doesn't exist
        let _ = tokio::fs::remove_file(&target_file).await;

        let records = vec![
            TestRecord {
                id: 1,
                name: "First".to_string(),
            },
            TestRecord {
                id: 2,
                name: "Second".to_string(),
            },
        ];

        write_jsonl_atomic(&target_file, &records).await.unwrap();

        // Verify file exists and has correct content
        assert!(target_file.exists());

        let mut file = File::open(&target_file).await.unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).await.unwrap();

        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        // Cleanup
        tokio::fs::remove_file(&target_file).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_replaces_existing_file() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_atomic_replace.jsonl");

        // Create initial file with old content
        tokio::fs::write(&target_file, "old content\n")
            .await
            .unwrap();

        let records = vec![TestRecord {
            id: 42,
            name: "New".to_string(),
        }];

        write_jsonl_atomic(&target_file, &records).await.unwrap();

        // Verify new content replaced old
        let mut file = File::open(&target_file).await.unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).await.unwrap();

        assert_eq!(contents.trim(), r#"{"id":42,"name":"New"}"#);

        // Cleanup
        tokio::fs::remove_file(&target_file).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_temp_file_cleaned_up_on_success() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_atomic_cleanup.jsonl");
        let temp_file = temp_dir.join("test_atomic_cleanup.jsonl.tmp");

        // Ensure files don't exist
        let _ = tokio::fs::remove_file(&target_file).await;
        let _ = tokio::fs::remove_file(&temp_file).await;

        let records = vec![TestRecord {
            id: 1,
            name: "Test".to_string(),
        }];

        write_jsonl_atomic(&target_file, &records).await.unwrap();

        // Target should exist, temp should not
        assert!(target_file.exists());
        assert!(!temp_file.exists());

        // Cleanup
        tokio::fs::remove_file(&target_file).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_empty_slice() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_atomic_empty.jsonl");

        // Ensure file doesn't exist
        let _ = tokio::fs::remove_file(&target_file).await;

        let records: Vec<TestRecord> = vec![];

        write_jsonl_atomic(&target_file, &records).await.unwrap();

        // Verify empty file was created
        assert!(target_file.exists());
        let metadata = tokio::fs::metadata(&target_file).await.unwrap();
        assert_eq!(metadata.len(), 0);

        // Cleanup
        tokio::fs::remove_file(&target_file).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_iter_with_generator() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_atomic_iter.jsonl");

        // Ensure file doesn't exist
        let _ = tokio::fs::remove_file(&target_file).await;

        // Use iterator instead of slice
        let records = (0..5).map(|id| TestRecord {
            id,
            name: format!("Record_{}", id),
        });

        write_jsonl_atomic_iter(&target_file, records)
            .await
            .unwrap();

        // Verify content
        let mut file = File::open(&target_file).await.unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).await.unwrap();

        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains(r#""id":0"#));
        assert!(lines[4].contains(r#""id":4"#));

        // Cleanup
        tokio::fs::remove_file(&target_file).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_large_batch() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_atomic_large.jsonl");

        // Ensure file doesn't exist
        let _ = tokio::fs::remove_file(&target_file).await;

        let records: Vec<TestRecord> = (0..1000)
            .map(|id| TestRecord {
                id,
                name: format!("Record_{}", id),
            })
            .collect();

        write_jsonl_atomic(&target_file, &records).await.unwrap();

        // Verify line count
        let mut file = File::open(&target_file).await.unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).await.unwrap();

        let line_count = contents.lines().count();
        assert_eq!(line_count, 1000);

        // Cleanup
        tokio::fs::remove_file(&target_file).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_unicode_content() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_atomic_unicode.jsonl");

        // Ensure file doesn't exist
        let _ = tokio::fs::remove_file(&target_file).await;

        let records = vec![TestRecord {
            id: 1,
            name: "Hello \u{4e16}\u{754c} \u{1F600}".to_string(),
        }];

        write_jsonl_atomic(&target_file, &records).await.unwrap();

        // Verify unicode preserved
        let mut file = File::open(&target_file).await.unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).await.unwrap();

        assert!(contents.contains("\u{4e16}\u{754c}"));
        assert!(contents.contains("\u{1F600}"));

        // Cleanup
        tokio::fs::remove_file(&target_file).await.unwrap();
    }
}
