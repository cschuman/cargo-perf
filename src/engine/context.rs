//! Analysis context and utilities for rule implementations.

use crate::Config;
use std::path::Path;

/// Pre-computed line index for O(log n) line/column lookups.
///
/// Instead of iterating from the start of the file for each lookup,
/// we build an index of line start positions once and use binary search.
pub struct LineIndex {
    /// Byte offsets where each line starts (0-indexed internally, 1-indexed for API)
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index from source text.
    ///
    /// Time: O(n) where n is the length of the source
    /// Space: O(lines) for storing line start positions
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0]; // Line 1 starts at byte 0

        for (i, c) in source.char_indices() {
            if c == '\n' {
                // Next line starts at the byte after the newline
                line_starts.push(i + 1);
            }
        }

        Self { line_starts }
    }

    /// Convert a byte offset to (line, column), both 1-indexed.
    ///
    /// Time: O(log n) where n is the number of lines
    ///
    /// # Panics
    /// Does not panic; returns the last valid position if offset is past end.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        // Binary search for the line containing this offset
        // partition_point returns the index where offset would be inserted
        // to maintain sorted order, which is one past the line we want
        let line_idx = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);

        let line = line_idx + 1; // Convert to 1-indexed
        let line_start = self.line_starts[line_idx];
        let column = offset.saturating_sub(line_start) + 1; // 1-indexed

        (line, column)
    }

    /// Get the byte offset where a line starts (1-indexed line number).
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line.saturating_sub(1)).copied()
    }

    /// Get the total number of lines.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Convert (line, column) to byte offset. Both are 1-indexed.
    ///
    /// Returns None if line is out of bounds.
    /// Column is clamped to line length if too large.
    pub fn byte_offset(&self, line: usize, column: usize) -> Option<usize> {
        let line_start = self.line_start(line)?;
        // Column is 1-indexed, so subtract 1
        Some(line_start + column.saturating_sub(1))
    }
}

/// Context passed to rules during analysis.
///
/// Contains all information needed to analyze a single file.
pub struct AnalysisContext<'a> {
    pub file_path: &'a Path,
    pub source: &'a str,
    pub ast: &'a syn::File,
    pub config: &'a Config,
    line_index: LineIndex,
}

impl<'a> AnalysisContext<'a> {
    /// Create a new analysis context.
    pub fn new(
        file_path: &'a Path,
        source: &'a str,
        ast: &'a syn::File,
        config: &'a Config,
    ) -> Self {
        Self {
            file_path,
            source,
            ast,
            config,
            line_index: LineIndex::new(source),
        }
    }

    /// Get line and column from a byte offset (1-indexed).
    ///
    /// This is O(log n) where n is the number of lines.
    #[inline]
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        self.line_index.line_col(offset)
    }

    /// Get the source line at the given line number (1-indexed).
    pub fn get_line(&self, line_num: usize) -> Option<&str> {
        self.source.lines().nth(line_num.saturating_sub(1))
    }

    /// Get a reference to the line index for advanced lookups.
    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    /// Convert a proc_macro2 span to byte range (start, end).
    ///
    /// Returns None if the span positions are invalid.
    pub fn span_to_byte_range(&self, span: proc_macro2::Span) -> Option<(usize, usize)> {
        let start = span.start();
        let end = span.end();

        let start_byte = self.line_index.byte_offset(start.line, start.column + 1)?;
        let end_byte = self.line_index.byte_offset(end.line, end.column + 1)?;

        Some((start_byte, end_byte))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index_simple() {
        let source = "line1\nline2\nline3";
        let index = LineIndex::new(source);

        assert_eq!(index.line_count(), 3);

        // First line
        assert_eq!(index.line_col(0), (1, 1)); // 'l'
        assert_eq!(index.line_col(4), (1, 5)); // '1'

        // Second line (after first newline at offset 5)
        assert_eq!(index.line_col(6), (2, 1)); // 'l'
        assert_eq!(index.line_col(10), (2, 5)); // '2'

        // Third line (after second newline at offset 11)
        assert_eq!(index.line_col(12), (3, 1)); // 'l'
    }

    #[test]
    fn test_line_index_empty() {
        let source = "";
        let index = LineIndex::new(source);

        assert_eq!(index.line_count(), 1);
        assert_eq!(index.line_col(0), (1, 1));
    }

    #[test]
    fn test_line_index_single_line() {
        let source = "hello world";
        let index = LineIndex::new(source);

        assert_eq!(index.line_count(), 1);
        assert_eq!(index.line_col(0), (1, 1));
        assert_eq!(index.line_col(5), (1, 6));
        assert_eq!(index.line_col(10), (1, 11));
    }

    #[test]
    fn test_line_index_trailing_newline() {
        let source = "line1\nline2\n";
        let index = LineIndex::new(source);

        assert_eq!(index.line_count(), 3); // Empty line 3 after trailing newline
        assert_eq!(index.line_col(12), (3, 1)); // Position after second newline
    }

    #[test]
    fn test_line_index_unicode() {
        let source = "héllo\nwörld";
        let index = LineIndex::new(source);

        // 'héllo' is 6 bytes (é is 2 bytes), newline at byte 6
        // 'wörld' starts at byte 7
        assert_eq!(index.line_col(0), (1, 1)); // 'h'
        assert_eq!(index.line_col(7), (2, 1)); // 'w'
    }

    #[test]
    fn test_line_start() {
        let source = "line1\nline2\nline3";
        let index = LineIndex::new(source);

        assert_eq!(index.line_start(1), Some(0));
        assert_eq!(index.line_start(2), Some(6));
        assert_eq!(index.line_start(3), Some(12));
        assert_eq!(index.line_start(4), None);
    }
}
