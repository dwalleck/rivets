//! JSONL reading operations.
//!
//! This module provides async functionality for reading JSONL files line-by-line
//! with efficient buffering and line number tracking for error reporting.

use crate::{Error, Result};
use futures::stream::Stream;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// Async reader for JSONL (JSON Lines) data.
///
/// `JsonlReader` wraps an async reader and provides buffered reading of JSONL
/// formatted data. It tracks line numbers to provide useful context in error
/// messages when parsing fails.
///
/// # Type Parameters
///
/// * `R` - The underlying async reader type. Must implement [`AsyncRead`] and [`Unpin`].
///
/// # Examples
///
/// ```no_run
/// use rivets_jsonl::reader::JsonlReader;
/// use tokio::fs::File;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let file = File::open("data.jsonl").await?;
/// let reader = JsonlReader::new(file);
/// // Use reader to parse JSONL data...
/// # Ok(())
/// # }
/// ```
pub struct JsonlReader<R> {
    /// Buffered reader wrapping the underlying async reader.
    reader: BufReader<R>,
    /// Current line number (1-based counting, 0 before any lines are read) for error reporting.
    line_number: usize,
}

impl<R: AsyncRead + Unpin> JsonlReader<R> {
    /// Creates a new `JsonlReader` wrapping the given async reader.
    ///
    /// The reader is wrapped in a [`BufReader`] for efficient buffered I/O.
    /// Line numbering uses 1-based indexing: the counter starts at 0 and increments
    /// after each line is read, so the first line read is numbered 1.
    ///
    /// # Arguments
    ///
    /// * `reader` - The underlying async reader to wrap.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rivets_jsonl::reader::JsonlReader;
    /// use tokio::fs::File;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::open("data.jsonl").await?;
    /// let reader = JsonlReader::new(file);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            line_number: 0,
        }
    }

    /// Creates a new `JsonlReader` with a custom buffer capacity.
    ///
    /// This is useful when you know the typical line length of your JSONL data
    /// and want to optimize buffer allocation.
    ///
    /// # Arguments
    ///
    /// * `reader` - The underlying async reader to wrap.
    /// * `capacity` - The initial buffer capacity in bytes.
    #[must_use]
    pub fn with_capacity(reader: R, capacity: usize) -> Self {
        Self {
            reader: BufReader::with_capacity(capacity, reader),
            line_number: 0,
        }
    }

    /// Returns the current line number.
    ///
    /// Returns 0 before any lines have been read. After reading, returns the
    /// 1-based line number of the last line read (first line = 1, second line = 2, etc.).
    #[must_use]
    pub fn line_number(&self) -> usize {
        self.line_number
    }

    /// Increments the line number counter.
    ///
    /// This should be called after successfully reading a line.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Will be used by read_line methods in future commits"
        )
    )]
    pub(crate) fn increment_line(&mut self) {
        self.line_number += 1;
    }

    /// Returns a reference to the underlying buffered reader.
    ///
    /// This provides access to the `BufReader` for advanced operations.
    #[must_use]
    pub fn get_ref(&self) -> &BufReader<R> {
        &self.reader
    }

    /// Returns a mutable reference to the underlying buffered reader.
    ///
    /// Use with caution: reading directly from the buffer may cause
    /// line number tracking to become inaccurate.
    pub fn get_mut(&mut self) -> &mut BufReader<R> {
        &mut self.reader
    }

    /// Consumes the reader, returning the underlying buffered reader.
    #[must_use]
    pub fn into_inner(self) -> BufReader<R> {
        self.reader
    }

    /// Reads a single line from the JSONL data and deserializes it.
    ///
    /// This method reads the next non-empty line, increments the line counter,
    /// and deserializes the JSON content into the specified type.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(T))` - Successfully read and deserialized a record
    /// - `Ok(None)` - End of file reached
    /// - `Err(Error::Io(_))` - I/O error during reading
    /// - `Err(Error::Json(_))` - JSON parsing error
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize each JSON line into. Must implement
    ///   [`DeserializeOwned`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rivets_jsonl::JsonlReader;
    /// use serde::Deserialize;
    /// use tokio::fs::File;
    ///
    /// #[derive(Deserialize)]
    /// struct Record {
    ///     id: u32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::open("data.jsonl").await?;
    /// let mut reader = JsonlReader::new(file);
    ///
    /// while let Some(record) = reader.read_line::<Record>().await? {
    ///     println!("Read record: {}", record.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_line<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                return Ok(None);
            }

            self.line_number += 1;

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let value: T = serde_json::from_str(trimmed)
                .map_err(|e| Error::InvalidFormat(format!("line {}: {}", self.line_number, e)))?;

            return Ok(Some(value));
        }
    }

    /// Returns an async [`Stream`] of deserialized records from the JSONL data.
    ///
    /// This method consumes the reader and returns a stream that lazily reads
    /// and deserializes records on demand. This enables efficient processing of
    /// large JSONL files with constant memory usage, as only one record is held
    /// in memory at a time.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize each JSON line into. Must implement
    ///   [`DeserializeOwned`] and `'static` (required by the stream implementation).
    ///
    /// # Returns
    ///
    /// An async stream that yields `Result<T>` for each line:
    /// - `Ok(T)` - Successfully read and deserialized a record
    /// - `Err(Error)` - I/O or JSON parsing error
    ///
    /// The stream terminates (returns `None`) when EOF is reached.
    ///
    /// # Stream Behavior
    ///
    /// - Uses [`futures::stream::unfold`] for lazy evaluation
    /// - Memory usage is constant regardless of file size
    /// - Errors are propagated through the stream (not short-circuited)
    /// - After an error, the stream continues attempting to read subsequent lines
    ///
    /// # When to Use
    ///
    /// - **Use `stream()`** for declarative stream processing with combinators
    ///   (filter, map, take, etc.) and when working with large files where constant
    ///   memory usage is important
    /// - **Use `read_line()`** for imperative-style processing with explicit
    ///   control flow and error handling, or when reading a small number of records
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rivets_jsonl::JsonlReader;
    /// use futures::stream::StreamExt;
    /// use serde::Deserialize;
    /// use std::pin::pin;
    /// use tokio::fs::File;
    ///
    /// #[derive(Deserialize)]
    /// struct Record {
    ///     id: u32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::open("data.jsonl").await?;
    /// let reader = JsonlReader::new(file);
    /// // Pin the stream to use with StreamExt methods
    /// let mut stream = pin!(reader.stream::<Record>());
    ///
    /// while let Some(result) = stream.next().await {
    ///     match result {
    ///         Ok(record) => println!("Read record: {}", record.name),
    ///         Err(e) => eprintln!("Error: {}", e),
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn stream<T>(self) -> impl Stream<Item = Result<T>>
    where
        T: DeserializeOwned + 'static,
    {
        futures::stream::unfold(self, |mut reader| async move {
            match reader.read_line().await {
                Ok(Some(value)) => Some((Ok(value), reader)),
                Ok(None) => None,
                Err(e) => Some((Err(e), reader)),
            }
        })
    }
}

impl<R: AsyncRead + Unpin + Default> Default for JsonlReader<R> {
    fn default() -> Self {
        Self::new(R::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::io::Cursor;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestRecord {
        id: u32,
        name: String,
    }

    #[test]
    fn new_reader_starts_at_line_zero() {
        let data = Cursor::new(b"");
        let reader = JsonlReader::new(data);
        assert_eq!(reader.line_number(), 0);
    }

    #[test]
    fn increment_line_increases_count() {
        let data = Cursor::new(b"");
        let mut reader = JsonlReader::new(data);
        assert_eq!(reader.line_number(), 0);

        reader.increment_line();
        assert_eq!(reader.line_number(), 1);

        reader.increment_line();
        assert_eq!(reader.line_number(), 2);
    }

    #[test]
    fn with_capacity_creates_reader() {
        let data = Cursor::new(b"test data");
        let reader = JsonlReader::with_capacity(data, 8192);
        assert_eq!(reader.line_number(), 0);
    }

    #[test]
    fn get_ref_returns_buffer_reference() {
        let data = Cursor::new(b"test");
        let reader = JsonlReader::new(data);
        let _buf_ref = reader.get_ref();
    }

    #[test]
    fn get_mut_returns_mutable_reference() {
        let data = Cursor::new(b"test");
        let mut reader = JsonlReader::new(data);
        let _buf_mut = reader.get_mut();
    }

    #[test]
    fn into_inner_returns_buffer() {
        let data = Cursor::new(b"test");
        let reader = JsonlReader::new(data);
        let _inner = reader.into_inner();
    }

    #[tokio::test]
    async fn read_line_returns_none_for_empty_input() {
        let data = Cursor::new(b"");
        let mut reader = JsonlReader::new(data);

        let result: Option<TestRecord> = reader.read_line().await.unwrap();
        assert!(result.is_none());
        assert_eq!(reader.line_number(), 0);
    }

    #[tokio::test]
    async fn read_line_reads_single_record() {
        let data = Cursor::new(b"{\"id\": 1, \"name\": \"Alice\"}\n");
        let mut reader = JsonlReader::new(data);

        let record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record.id, 1);
        assert_eq!(record.name, "Alice");
        assert_eq!(reader.line_number(), 1);

        let result: Option<TestRecord> = reader.read_line().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_line_reads_multiple_records() {
        let data = Cursor::new(
            b"{\"id\": 1, \"name\": \"Alice\"}\n{\"id\": 2, \"name\": \"Bob\"}\n{\"id\": 3, \"name\": \"Charlie\"}\n",
        );
        let mut reader = JsonlReader::new(data);

        let record1: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record1.id, 1);
        assert_eq!(record1.name, "Alice");
        assert_eq!(reader.line_number(), 1);

        let record2: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record2.id, 2);
        assert_eq!(record2.name, "Bob");
        assert_eq!(reader.line_number(), 2);

        let record3: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record3.id, 3);
        assert_eq!(record3.name, "Charlie");
        assert_eq!(reader.line_number(), 3);

        let result: Option<TestRecord> = reader.read_line().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_line_skips_empty_lines() {
        let data = Cursor::new(
            b"\n{\"id\": 1, \"name\": \"Alice\"}\n\n\n{\"id\": 2, \"name\": \"Bob\"}\n",
        );
        let mut reader = JsonlReader::new(data);

        let record1: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record1.id, 1);
        assert_eq!(reader.line_number(), 2);

        let record2: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record2.id, 2);
        assert_eq!(reader.line_number(), 5);

        let result: Option<TestRecord> = reader.read_line().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_line_skips_whitespace_only_lines() {
        let data = Cursor::new(b"   \n{\"id\": 1, \"name\": \"Alice\"}\n\t\t\n");
        let mut reader = JsonlReader::new(data);

        let record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record.id, 1);
        assert_eq!(reader.line_number(), 2);

        let result: Option<TestRecord> = reader.read_line().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_line_handles_line_without_trailing_newline() {
        let data = Cursor::new(b"{\"id\": 1, \"name\": \"Alice\"}");
        let mut reader = JsonlReader::new(data);

        let record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record.id, 1);
        assert_eq!(reader.line_number(), 1);

        let result: Option<TestRecord> = reader.read_line().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_line_returns_error_for_invalid_json() {
        let data = Cursor::new(b"{invalid json}\n");
        let mut reader = JsonlReader::new(data);

        let result: Result<Option<TestRecord>> = reader.read_line().await;
        assert!(result.is_err());
        assert_eq!(reader.line_number(), 1);

        let error = result.unwrap_err();
        match error {
            Error::InvalidFormat(msg) => {
                assert!(msg.contains("line 1"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[tokio::test]
    async fn read_line_returns_error_for_type_mismatch() {
        let data = Cursor::new(b"{\"id\": \"not a number\", \"name\": \"Alice\"}\n");
        let mut reader = JsonlReader::new(data);

        let result: Result<Option<TestRecord>> = reader.read_line().await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            Error::InvalidFormat(msg) => {
                assert!(msg.contains("line 1"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[tokio::test]
    async fn read_line_includes_correct_line_number_in_error() {
        let data = Cursor::new(b"{\"id\": 1, \"name\": \"Alice\"}\n{invalid}\n");
        let mut reader = JsonlReader::new(data);

        let _record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(reader.line_number(), 1);

        let result: Result<Option<TestRecord>> = reader.read_line().await;
        assert!(result.is_err());
        assert_eq!(reader.line_number(), 2);

        let error = result.unwrap_err();
        match error {
            Error::InvalidFormat(msg) => {
                assert!(msg.contains("line 2"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[tokio::test]
    async fn read_line_trims_leading_and_trailing_whitespace() {
        let data = Cursor::new(b"  {\"id\": 1, \"name\": \"Alice\"}  \n");
        let mut reader = JsonlReader::new(data);

        let record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record.id, 1);
        assert_eq!(record.name, "Alice");
    }

    #[tokio::test]
    async fn read_line_handles_only_empty_lines() {
        let data = Cursor::new(b"\n\n\n");
        let mut reader = JsonlReader::new(data);

        let result: Option<TestRecord> = reader.read_line().await.unwrap();
        assert!(result.is_none());
        assert_eq!(reader.line_number(), 3);
    }

    // ============================================================================
    // Stream tests
    // ============================================================================

    mod stream_tests {
        use super::*;
        use futures::stream::StreamExt;
        use std::pin::pin;

        #[tokio::test]
        async fn stream_returns_empty_for_empty_input() {
            let data = Cursor::new(b"");
            let reader = JsonlReader::new(data);
            let mut stream = pin!(reader.stream::<TestRecord>());

            let result = stream.next().await;
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn stream_reads_single_record() {
            let data = Cursor::new(b"{\"id\": 1, \"name\": \"Alice\"}\n");
            let reader = JsonlReader::new(data);
            let mut stream = pin!(reader.stream::<TestRecord>());

            let record = stream.next().await.unwrap().unwrap();
            assert_eq!(record.id, 1);
            assert_eq!(record.name, "Alice");

            let result = stream.next().await;
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn stream_reads_multiple_records() {
            let data = Cursor::new(
                b"{\"id\": 1, \"name\": \"Alice\"}\n{\"id\": 2, \"name\": \"Bob\"}\n{\"id\": 3, \"name\": \"Charlie\"}\n",
            );
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<TestRecord>());

            let records: Vec<TestRecord> = stream.map(|r| r.unwrap()).collect().await;

            assert_eq!(records.len(), 3);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[0].name, "Alice");
            assert_eq!(records[1].id, 2);
            assert_eq!(records[1].name, "Bob");
            assert_eq!(records[2].id, 3);
            assert_eq!(records[2].name, "Charlie");
        }

        #[tokio::test]
        async fn stream_skips_empty_lines() {
            let data = Cursor::new(
                b"\n{\"id\": 1, \"name\": \"Alice\"}\n\n\n{\"id\": 2, \"name\": \"Bob\"}\n",
            );
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<TestRecord>());

            let records: Vec<TestRecord> = stream.map(|r| r.unwrap()).collect().await;

            assert_eq!(records.len(), 2);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[1].id, 2);
        }

        #[tokio::test]
        async fn stream_propagates_errors() {
            let data = Cursor::new(b"{\"id\": 1, \"name\": \"Alice\"}\n{invalid}\n");
            let reader = JsonlReader::new(data);
            let mut stream = pin!(reader.stream::<TestRecord>());

            let record = stream.next().await.unwrap().unwrap();
            assert_eq!(record.id, 1);

            let error = stream.next().await.unwrap();
            assert!(error.is_err());
            match error.unwrap_err() {
                Error::InvalidFormat(msg) => {
                    assert!(msg.contains("line 2"));
                }
                _ => panic!("Expected InvalidFormat error"),
            }

            let result = stream.next().await;
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn stream_continues_after_error() {
            let data = Cursor::new(
                b"{\"id\": 1, \"name\": \"Alice\"}\n{invalid}\n{\"id\": 3, \"name\": \"Charlie\"}\n",
            );
            let reader = JsonlReader::new(data);
            let mut stream = pin!(reader.stream::<TestRecord>());

            let record1 = stream.next().await.unwrap().unwrap();
            assert_eq!(record1.id, 1);

            let error = stream.next().await.unwrap();
            assert!(error.is_err());

            let record3 = stream.next().await.unwrap().unwrap();
            assert_eq!(record3.id, 3);
            assert_eq!(record3.name, "Charlie");

            let result = stream.next().await;
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn stream_handles_line_without_trailing_newline() {
            let data = Cursor::new(b"{\"id\": 1, \"name\": \"Alice\"}");
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<TestRecord>());

            let records: Vec<TestRecord> = stream.map(|r| r.unwrap()).collect().await;

            assert_eq!(records.len(), 1);
            assert_eq!(records[0].id, 1);
        }

        #[tokio::test]
        async fn stream_can_be_collected() {
            let data =
                Cursor::new(b"{\"id\": 1, \"name\": \"Alice\"}\n{\"id\": 2, \"name\": \"Bob\"}\n");
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<TestRecord>());

            let results: Vec<Result<TestRecord>> = stream.collect().await;

            assert_eq!(results.len(), 2);
            assert!(results.iter().all(|r| r.is_ok()));
        }

        #[tokio::test]
        async fn stream_can_use_take() {
            let data = Cursor::new(
                b"{\"id\": 1, \"name\": \"Alice\"}\n{\"id\": 2, \"name\": \"Bob\"}\n{\"id\": 3, \"name\": \"Charlie\"}\n",
            );
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<TestRecord>());

            let records: Vec<TestRecord> = stream.take(2).map(|r| r.unwrap()).collect().await;

            assert_eq!(records.len(), 2);
            assert_eq!(records[0].id, 1);
            assert_eq!(records[1].id, 2);
        }

        #[tokio::test]
        async fn stream_can_use_filter() {
            let data = Cursor::new(
                b"{\"id\": 1, \"name\": \"Alice\"}\n{\"id\": 2, \"name\": \"Bob\"}\n{\"id\": 3, \"name\": \"Charlie\"}\n",
            );
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<TestRecord>());

            let records: Vec<TestRecord> = stream
                .filter_map(|r| async move { r.ok() })
                .filter(|r| {
                    let matches = r.id > 1;
                    async move { matches }
                })
                .collect()
                .await;

            assert_eq!(records.len(), 2);
            assert_eq!(records[0].id, 2);
            assert_eq!(records[1].id, 3);
        }

        #[tokio::test]
        async fn stream_is_lazy_evaluated() {
            use std::sync::atomic::{AtomicUsize, Ordering};
            use std::sync::Arc;

            #[derive(Debug, Deserialize)]
            #[expect(dead_code, reason = "Field used for JSON deserialization only")]
            struct CountingRecord {
                value: u32,
            }

            let data = Cursor::new(
                b"{\"value\": 1}\n{\"value\": 2}\n{\"value\": 3}\n{\"value\": 4}\n{\"value\": 5}\n",
            );
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<CountingRecord>());

            let count = Arc::new(AtomicUsize::new(0));
            let count_clone = count.clone();

            let records: Vec<CountingRecord> = stream
                .take(2)
                .inspect(|_| {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                })
                .filter_map(|r| async move { r.ok() })
                .collect()
                .await;

            assert_eq!(records.len(), 2);
            assert_eq!(count.load(Ordering::SeqCst), 2);
        }

        #[tokio::test]
        async fn stream_handles_whitespace_only_lines() {
            let data = Cursor::new(b"   \n{\"id\": 1, \"name\": \"Alice\"}\n\t\t\n");
            let reader = JsonlReader::new(data);
            let stream = pin!(reader.stream::<TestRecord>());

            let records: Vec<TestRecord> = stream.map(|r| r.unwrap()).collect().await;

            assert_eq!(records.len(), 1);
            assert_eq!(records[0].id, 1);
        }
    }
}
