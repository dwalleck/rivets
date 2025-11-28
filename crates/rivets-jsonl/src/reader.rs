//! JSONL reading operations.
//!
//! This module provides async functionality for reading JSONL files line-by-line
//! with efficient buffering and line number tracking for error reporting.

use tokio::io::{AsyncRead, BufReader};

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
}

impl<R: AsyncRead + Unpin + Default> Default for JsonlReader<R> {
    fn default() -> Self {
        Self::new(R::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
}
