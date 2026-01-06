use crate::Config;
use std::path::Path;

/// Context passed to rules during analysis
pub struct AnalysisContext<'a> {
    pub file_path: &'a Path,
    pub source: &'a str,
    pub ast: &'a syn::File,
    pub config: &'a Config,
}

impl<'a> AnalysisContext<'a> {
    /// Get line and column from a byte offset
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;
        for (i, c) in self.source.char_indices() {
            if i >= offset {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Get the source line at the given line number (1-indexed)
    pub fn get_line(&self, line_num: usize) -> Option<&str> {
        self.source.lines().nth(line_num.saturating_sub(1))
    }
}
