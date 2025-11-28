//! JSONL writing operations.
//!
//! This module provides async functionality for writing data in JSONL format
//! with efficient buffering.

use tokio::io::{AsyncWrite, BufWriter};

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
}

impl<W: AsyncWrite + Unpin + Default> Default for JsonlWriter<W> {
    fn default() -> Self {
        Self::new(W::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
}
