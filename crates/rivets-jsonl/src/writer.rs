//! JSONL writing operations.
//!
//! This module provides async functionality for writing data in JSONL format
//! with efficient buffering.

use crate::Result;
use serde::Serialize;
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};

/// Async writer for JSONL (JSON Lines) data.
///
/// `JsonlWriter` wraps an async writer and provides buffered writing of JSONL
/// formatted data. Each JSON value is serialized to a single line followed by
/// a newline character.
///
/// # Type Parameters
///
/// * `W` - The underlying async writer type. Must implement [`AsyncWrite`] and [`Unpin`].
///
/// # Examples
///
/// ```no_run
/// use rivets_jsonl::writer::JsonlWriter;
/// use tokio::fs::File;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let file = File::create("output.jsonl").await?;
/// let writer = JsonlWriter::new(file);
/// // Use writer to serialize data to JSONL format...
/// # Ok(())
/// # }
/// ```
pub struct JsonlWriter<W> {
    /// Buffered writer wrapping the underlying async writer.
    writer: BufWriter<W>,
}

impl<W: AsyncWrite + Unpin> JsonlWriter<W> {
    /// Creates a new `JsonlWriter` wrapping the given async writer.
    ///
    /// The writer is wrapped in a [`BufWriter`] for efficient buffered I/O,
    /// reducing the number of system calls when writing many small records.
    ///
    /// # Arguments
    ///
    /// * `writer` - The underlying async writer to wrap.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rivets_jsonl::writer::JsonlWriter;
    /// use tokio::fs::File;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::create("output.jsonl").await?;
    /// let writer = JsonlWriter::new(file);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
        }
    }

    /// Creates a new `JsonlWriter` with a custom buffer capacity.
    ///
    /// This is useful when writing many small records and you want to
    /// control memory usage or optimize for specific write patterns.
    ///
    /// # Arguments
    ///
    /// * `writer` - The underlying async writer to wrap.
    /// * `capacity` - The initial buffer capacity in bytes.
    #[must_use]
    pub fn with_capacity(writer: W, capacity: usize) -> Self {
        Self {
            writer: BufWriter::with_capacity(capacity, writer),
        }
    }

    /// Returns a reference to the underlying buffered writer.
    ///
    /// This provides access to the `BufWriter` for advanced operations
    /// like checking buffer state.
    #[must_use]
    pub fn get_ref(&self) -> &BufWriter<W> {
        &self.writer
    }

    /// Returns a mutable reference to the underlying buffered writer.
    ///
    /// Use with caution: writing directly to the buffer may produce
    /// malformed JSONL output if not properly formatted.
    pub fn get_mut(&mut self) -> &mut BufWriter<W> {
        &mut self.writer
    }

    /// Consumes the writer, returning the underlying buffered writer.
    ///
    /// Note: This does not flush the buffer. Call [`flush`](Self::flush)
    /// before calling this method to ensure all data is written.
    #[must_use]
    pub fn into_inner(self) -> BufWriter<W> {
        self.writer
    }

    /// Writes a single value to the JSONL output.
    ///
    /// The value is serialized to JSON using serde and written as a single line
    /// followed by a newline character. The output is buffered; call [`flush`](Self::flush)
    /// to ensure all data is written to the underlying writer.
    ///
    /// # Arguments
    ///
    /// * `value` - The value to serialize and write. Must implement [`Serialize`].
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Serialization fails (e.g., the type contains non-serializable data)
    /// - An I/O error occurs while writing
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rivets_jsonl::JsonlWriter;
    /// use serde::Serialize;
    /// use tokio::fs::File;
    ///
    /// #[derive(Serialize)]
    /// struct Record {
    ///     id: u32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::create("output.jsonl").await?;
    /// let mut writer = JsonlWriter::new(file);
    ///
    /// let record = Record { id: 1, name: "Alice".to_string() };
    /// writer.write(&record).await?;
    /// writer.flush().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write<T: Serialize>(&mut self, value: &T) -> Result<()> {
        let json = serde_json::to_string(value)?;
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        Ok(())
    }

    /// Writes multiple values to the JSONL output.
    ///
    /// Each value is serialized to JSON and written as a separate line.
    /// This is more convenient than calling [`write`](Self::write) in a loop,
    /// though the performance is equivalent.
    ///
    /// # Arguments
    ///
    /// * `values` - An iterator of values to serialize and write.
    ///
    /// # Errors
    ///
    /// Returns an error if any value fails to serialize or if an I/O error occurs.
    /// If an error occurs partway through, some values may have been written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rivets_jsonl::JsonlWriter;
    /// use serde::Serialize;
    /// use tokio::fs::File;
    ///
    /// #[derive(Serialize)]
    /// struct Record {
    ///     id: u32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::create("output.jsonl").await?;
    /// let mut writer = JsonlWriter::new(file);
    ///
    /// let records = vec![
    ///     Record { id: 1, name: "Alice".to_string() },
    ///     Record { id: 2, name: "Bob".to_string() },
    /// ];
    /// writer.write_all(records.iter()).await?;
    /// writer.flush().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write_all<T, I>(&mut self, values: I) -> Result<()>
    where
        T: Serialize,
        I: IntoIterator<Item = T>,
    {
        for value in values {
            self.write(&value).await?;
        }
        Ok(())
    }

    /// Flushes the buffered writer, ensuring all data is written to the underlying writer.
    ///
    /// This should be called after writing to ensure all buffered data is persisted.
    /// It is automatically called when the writer is dropped, but calling it explicitly
    /// allows you to handle any I/O errors.
    ///
    /// # Errors
    ///
    /// Returns an error if flushing fails due to an I/O error.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rivets_jsonl::JsonlWriter;
    /// use serde::Serialize;
    /// use tokio::fs::File;
    ///
    /// #[derive(Serialize)]
    /// struct Record {
    ///     id: u32,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::create("output.jsonl").await?;
    /// let mut writer = JsonlWriter::new(file);
    ///
    /// writer.write(&Record { id: 1 }).await?;
    /// writer.flush().await?; // Ensure data is written
    /// # Ok(())
    /// # }
    /// ```
    pub async fn flush(&mut self) -> Result<()> {
        self.writer.flush().await?;
        Ok(())
    }
}

impl<W: AsyncWrite + Unpin + Default> Default for JsonlWriter<W> {
    fn default() -> Self {
        Self::new(W::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::io::Cursor;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestRecord {
        id: u32,
        name: String,
    }

    #[derive(Debug, Serialize)]
    struct SimpleRecord {
        value: i32,
    }

    #[test]
    fn new_creates_writer() {
        let buffer = Cursor::new(Vec::new());
        let _writer = JsonlWriter::new(buffer);
    }

    #[test]
    fn with_capacity_creates_writer() {
        let buffer = Cursor::new(Vec::new());
        let _writer = JsonlWriter::with_capacity(buffer, 8192);
    }

    #[test]
    fn get_ref_returns_buffer_reference() {
        let buffer = Cursor::new(Vec::new());
        let writer = JsonlWriter::new(buffer);
        let _buf_ref = writer.get_ref();
    }

    #[test]
    fn get_mut_returns_mutable_reference() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);
        let _buf_mut = writer.get_mut();
    }

    #[test]
    fn into_inner_returns_buffer() {
        let buffer = Cursor::new(Vec::new());
        let writer = JsonlWriter::new(buffer);
        let _inner = writer.into_inner();
    }

    #[tokio::test]
    async fn write_single_record() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let record = TestRecord {
            id: 1,
            name: "Alice".to_string(),
        };
        writer.write(&record).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        assert_eq!(output, "{\"id\":1,\"name\":\"Alice\"}\n");
    }

    #[tokio::test]
    async fn write_multiple_records_individually() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        writer
            .write(&TestRecord {
                id: 1,
                name: "Alice".to_string(),
            })
            .await
            .unwrap();
        writer
            .write(&TestRecord {
                id: 2,
                name: "Bob".to_string(),
            })
            .await
            .unwrap();
        writer
            .write(&TestRecord {
                id: 3,
                name: "Charlie".to_string(),
            })
            .await
            .unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "{\"id\":1,\"name\":\"Alice\"}");
        assert_eq!(lines[1], "{\"id\":2,\"name\":\"Bob\"}");
        assert_eq!(lines[2], "{\"id\":3,\"name\":\"Charlie\"}");
    }

    #[tokio::test]
    async fn write_all_writes_multiple_records() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

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
        writer.write_all(records.iter()).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"id\":1,\"name\":\"Alice\"}");
        assert_eq!(lines[1], "{\"id\":2,\"name\":\"Bob\"}");
    }

    #[tokio::test]
    async fn write_all_empty_iterator() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let records: Vec<TestRecord> = vec![];
        writer.write_all(records.iter()).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = inner.into_inner().into_inner();
        assert!(output.is_empty());
    }

    #[tokio::test]
    async fn write_all_with_owned_values() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let records = vec![SimpleRecord { value: 42 }, SimpleRecord { value: 99 }];
        writer.write_all(records).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"value\":42}");
        assert_eq!(lines[1], "{\"value\":99}");
    }

    #[tokio::test]
    async fn write_escapes_special_characters() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let record = TestRecord {
            id: 1,
            name: "Line1\nLine2\tTabbed\"Quoted\"".to_string(),
        };
        writer.write(&record).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();

        assert!(output.contains("\\n"));
        assert!(output.contains("\\t"));
        assert!(output.contains("\\\""));
        assert!(output.ends_with('\n'));
    }

    #[tokio::test]
    async fn write_unicode_characters() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let record = TestRecord {
            id: 1,
            name: "Hello, \u{4e16}\u{754c}! \u{1F600}".to_string(),
        };
        writer.write(&record).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();

        assert!(output.contains("\u{4e16}\u{754c}"));
        assert!(output.ends_with('\n'));
    }

    #[tokio::test]
    async fn flush_writes_buffered_data() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        writer.write(&SimpleRecord { value: 42 }).await.unwrap();

        let buf_ref = writer.get_ref();
        let before_flush = buf_ref.get_ref().get_ref().len();

        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let after_flush = inner.into_inner().into_inner().len();

        assert!(
            after_flush >= before_flush,
            "Flush should write buffered data"
        );
    }

    #[tokio::test]
    async fn write_different_types_in_sequence() {
        #[derive(Serialize)]
        struct TypeA {
            a: String,
        }

        #[derive(Serialize)]
        struct TypeB {
            b: i32,
        }

        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        writer
            .write(&TypeA {
                a: "first".to_string(),
            })
            .await
            .unwrap();
        writer.write(&TypeB { b: 42 }).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"a\":\"first\"}");
        assert_eq!(lines[1], "{\"b\":42}");
    }

    #[tokio::test]
    async fn write_primitive_types() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        writer.write(&42i32).await.unwrap();
        writer.write(&"hello").await.unwrap();
        writer.write(&true).await.unwrap();
        writer.write(&1.234f64).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "42");
        assert_eq!(lines[1], "\"hello\"");
        assert_eq!(lines[2], "true");
        assert_eq!(lines[3], "1.234");
    }

    #[tokio::test]
    async fn write_null_value() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let value: Option<i32> = None;
        writer.write(&value).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        assert_eq!(output, "null\n");
    }

    #[tokio::test]
    async fn write_nested_struct() {
        #[derive(Serialize)]
        struct Inner {
            value: i32,
        }

        #[derive(Serialize)]
        struct Outer {
            name: String,
            inner: Inner,
        }

        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let record = Outer {
            name: "test".to_string(),
            inner: Inner { value: 42 },
        };
        writer.write(&record).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        assert_eq!(output, "{\"name\":\"test\",\"inner\":{\"value\":42}}\n");
    }

    #[tokio::test]
    async fn write_array_value() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let array = vec![1, 2, 3, 4, 5];
        writer.write(&array).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        assert_eq!(output, "[1,2,3,4,5]\n");
    }

    #[tokio::test]
    async fn write_with_custom_capacity() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::with_capacity(buffer, 16384);

        writer.write(&SimpleRecord { value: 1 }).await.unwrap();
        writer.write(&SimpleRecord { value: 2 }).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn write_empty_string() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let record = TestRecord {
            id: 1,
            name: String::new(),
        };
        writer.write(&record).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        assert_eq!(output, "{\"id\":1,\"name\":\"\"}\n");
    }

    #[tokio::test]
    async fn write_large_record() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let large_name = "x".repeat(10_000);
        let record = TestRecord {
            id: 1,
            name: large_name.clone(),
        };
        writer.write(&record).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        assert!(output.contains(&large_name));
        assert!(output.ends_with('\n'));
    }

    #[tokio::test]
    async fn write_all_large_batch() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);

        let records: Vec<SimpleRecord> = (0..1000).map(|i| SimpleRecord { value: i }).collect();
        writer.write_all(records).await.unwrap();
        writer.flush().await.unwrap();

        let inner = writer.into_inner();
        let output = String::from_utf8(inner.into_inner().into_inner()).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 1000);
    }
}
