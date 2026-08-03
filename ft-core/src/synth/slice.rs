//! Line-range slicing shared by every protected-section consumer.
//!
//! `count_lines` and `slice_lines` are the single definition of "how
//! many lines does this text have" and "what is the body at a
//! 1-indexed inclusive range" — used by the emitter (`ft notes quote`,
//! working-tree side), the verifier (`verify.rs`, git-blob side), and
//! the re-slicers (`reslice.rs`, `repair.rs`). One convention
//! everywhere guarantees that what the emitter produces always
//! verifies `ok`.

/// Number of lines in `content`. A trailing newline is not an extra
/// line: `"a\nb\n"` has 2 lines, `""` has 0.
pub fn count_lines(content: &str) -> u32 {
    if content.is_empty() {
        return 0;
    }
    // `split('\n')` yields a trailing empty element when `content`
    // ends with '\n'; that element is not a line.
    if content.ends_with('\n') {
        content.split('\n').count().saturating_sub(1) as u32
    } else {
        content.split('\n').count() as u32
    }
}

/// Slice `content`'s lines `line_start..=line_end` (1-indexed
/// inclusive), rejoined with `\n` and no trailing newline. Returns
/// `None` when the range is empty or out of bounds — validate against
/// [`count_lines`] when you need the actual count for a diagnostic.
pub fn slice_lines(content: &str, line_start: u32, line_end: u32) -> Option<String> {
    if line_start < 1 || line_end < line_start {
        return None;
    }
    let n = count_lines(content);
    if line_end > n {
        return None;
    }
    let lines: Vec<&str> = content.split('\n').collect();
    Some(lines[(line_start as usize - 1)..line_end as usize].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_lines_basics() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("a"), 1);
        assert_eq!(count_lines("a\n"), 1);
        assert_eq!(count_lines("a\nb"), 2);
        assert_eq!(count_lines("a\nb\n"), 2);
        assert_eq!(count_lines("a\n\nb\n"), 3);
    }

    #[test]
    fn multi_line_slice() {
        assert_eq!(slice_lines("a\nb\nc\nd\n", 2, 3), Some("b\nc".to_string()));
    }

    #[test]
    fn single_line_slice() {
        assert_eq!(slice_lines("a\nb\nc", 2, 2), Some("b".to_string()));
    }

    #[test]
    fn full_file_slice() {
        assert_eq!(slice_lines("a\nb\n", 1, 2), Some("a\nb".to_string()));
        assert_eq!(slice_lines("a\nb", 1, 2), Some("a\nb".to_string()));
    }

    #[test]
    fn trailing_newline_is_not_a_line() {
        // "a\nb\n" has 2 lines; L1-2 is the whole file, no phantom line.
        assert_eq!(slice_lines("a\nb\n", 1, 2), Some("a\nb".to_string()));
        // L1-3 would reach the phantom empty element → out of bounds.
        assert_eq!(slice_lines("a\nb\n", 1, 3), None);
        assert_eq!(slice_lines("a\nb", 1, 2), Some("a\nb".to_string()));
        assert_eq!(slice_lines("a\nb", 1, 3), None);
    }

    #[test]
    fn empty_file_rejects_any_range() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(slice_lines("", 1, 1), None);
    }

    #[test]
    fn zero_start_rejected() {
        assert_eq!(slice_lines("a\nb", 0, 1), None);
    }

    #[test]
    fn start_after_end_rejected() {
        assert_eq!(slice_lines("a\nb", 2, 1), None);
    }

    #[test]
    fn end_past_line_count_rejected() {
        assert_eq!(slice_lines("a\nb", 1, 3), None);
        assert_eq!(slice_lines("a\nb\n", 2, 3), None);
    }

    #[test]
    fn embedded_blank_line_preserved() {
        // Lines 2-3 span a blank line: body keeps the double newline.
        assert_eq!(slice_lines("a\n\nb\n", 2, 3), Some("\nb".to_string()));
    }
}
