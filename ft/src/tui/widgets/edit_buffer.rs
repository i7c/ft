//! Single-line edit buffer with char-precise cursor handling. Used by every
//! TUI surface that takes typed input (search query bar, edit popup fields,
//! the new-task quickline, the fuzzy picker).
//!
//! The buffer stores a `String` plus a *character* cursor (not a byte
//! offset) so the math stays simple for multi-byte glyphs. Methods are
//! deliberately tiny so callers can compose readline-style behavior
//! (Ctrl+W word delete, Home/End jump, etc.) without forking the type.
//!
//! Word boundaries follow a uniform `[A-Za-z0-9_]` rule: a word is a
//! maximal run of those chars. Every word-aware operation
//! (`delete_word_backward`, `move_word_back`, `move_word_forward`,
//! `kill_word_back`, `kill_word_forward`) uses the same definition.
//! This is a deliberate change from the pre-`text-input-ux` behaviour
//! of `delete_word_backward`, which used whitespace boundaries.
//!
//! A single-slot kill ring backs the kill/yank operations (`Ctrl+K`,
//! `Ctrl+U`, `Ctrl+W`, `Alt+D` populate it; `Ctrl+Y` reads it). Each
//! kill replaces the previous contents; `yank` does not clear the ring,
//! so multiple yanks insert multiple copies.

/// Returns true iff `c` is a "word char" under this buffer's word
/// rule: ASCII alphanumeric or `_`. Whitespace and punctuation are
/// non-word.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[derive(Debug, Clone, Default)]
pub struct EditBuffer {
    pub text: String,
    /// Cursor position as a character offset (not byte offset).
    pub cursor: usize,
    /// Last killed text. Replaced on each kill operation; read by
    /// `yank`. `None` if nothing has been killed since the buffer was
    /// constructed (or since the ring was cleared).
    pub kill_ring: Option<String>,
}

impl EditBuffer {
    pub fn from(text: &str) -> Self {
        let cursor = text.chars().count();
        Self {
            text: text.to_string(),
            cursor,
            kill_ring: None,
        }
    }

    /// Byte offset corresponding to the char index `char_idx`. Returns
    /// `text.len()` for `char_idx >= text.chars().count()`.
    fn byte_of_char(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len())
    }

    /// Total char count of the buffer's text.
    fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn insert(&mut self, c: char) {
        let byte_idx = self.byte_of_char(self.cursor);
        self.text.insert(byte_idx, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_char = self
            .text
            .char_indices()
            .nth(self.cursor - 1)
            .map(|(b, c)| (b, c.len_utf8()));
        if let Some((b, len)) = prev_char {
            self.text.replace_range(b..b + len, "");
            self.cursor -= 1;
        }
    }

    pub fn delete(&mut self) {
        let target = self
            .text
            .char_indices()
            .nth(self.cursor)
            .map(|(b, c)| (b, c.len_utf8()));
        if let Some((b, len)) = target {
            self.text.replace_range(b..b + len, "");
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        if self.cursor < self.char_len() {
            self.cursor += 1;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.char_len();
    }

    // ── Word jumps ───────────────────────────────────────────────────

    /// Move the cursor one word back. Skips non-word chars before the
    /// cursor, then skips word chars; lands on the first char of the
    /// word (or 0 if the buffer has no word chars before the cursor).
    pub fn move_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.cursor;
        while i > 0 && !is_word_char(chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word_char(chars[i - 1]) {
            i -= 1;
        }
        self.cursor = i;
    }

    /// Move the cursor one word forward. Skips non-word chars at the
    /// cursor, then skips word chars; lands one past the end of the
    /// word.
    pub fn move_word_forward(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let n = chars.len();
        let mut i = self.cursor;
        while i < n && !is_word_char(chars[i]) {
            i += 1;
        }
        while i < n && is_word_char(chars[i]) {
            i += 1;
        }
        self.cursor = i;
    }

    // ── Kills ────────────────────────────────────────────────────────

    /// Remove `[start_char..end_char)` from the text, save it to the
    /// kill ring, and reset the cursor to `start_char`. No-op for an
    /// empty range.
    fn kill_range(&mut self, start_char: usize, end_char: usize) {
        if start_char >= end_char {
            return;
        }
        let start_byte = self.byte_of_char(start_char);
        let end_byte = self.byte_of_char(end_char);
        let killed: String = self.text[start_byte..end_byte].to_string();
        self.text.replace_range(start_byte..end_byte, "");
        self.kill_ring = Some(killed);
        self.cursor = start_char;
    }

    /// Delete from the cursor to end-of-line; save the killed text to
    /// the kill ring. Cursor stays put.
    pub fn kill_to_end(&mut self) {
        let end = self.char_len();
        self.kill_range(self.cursor, end);
    }

    /// Delete from start-of-line to the cursor; save to kill ring.
    /// Cursor moves to 0.
    pub fn kill_to_start(&mut self) {
        self.kill_range(0, self.cursor);
    }

    /// Delete the word before the cursor and save it to the kill ring.
    /// Word boundary: `[A-Za-z0-9_]`. Skips trailing non-word chars
    /// before erasing.
    pub fn kill_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.cursor;
        while i > 0 && !is_word_char(chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word_char(chars[i - 1]) {
            i -= 1;
        }
        self.kill_range(i, self.cursor);
    }

    /// Delete the word after the cursor and save it to the kill ring.
    pub fn kill_word_forward(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let n = chars.len();
        if self.cursor >= n {
            return;
        }
        let mut i = self.cursor;
        while i < n && !is_word_char(chars[i]) {
            i += 1;
        }
        while i < n && is_word_char(chars[i]) {
            i += 1;
        }
        self.kill_range(self.cursor, i);
    }

    /// Delete the word before the cursor (without involving the kill
    /// ring's read side — but, post-`text-input-ux`, kills *do* populate
    /// the ring so `Ctrl+Y` can recover the loss). Word boundary is the
    /// shared `[A-Za-z0-9_]` rule. Pre-`text-input-ux` this used
    /// whitespace boundaries; e.g. `foo.bar.baz` was one delete, now
    /// it's three.
    pub fn delete_word_backward(&mut self) {
        self.kill_word_back();
    }

    // ── Yank ─────────────────────────────────────────────────────────

    /// Insert the kill ring's contents at the cursor and advance the
    /// cursor past the inserted text. No-op if the kill ring is empty.
    /// The ring is not cleared — repeated yanks insert repeated copies.
    pub fn yank(&mut self) {
        let Some(s) = self.kill_ring.as_ref() else {
            return;
        };
        if s.is_empty() {
            return;
        }
        let inserted = s.clone();
        let byte_idx = self.byte_of_char(self.cursor);
        self.text.insert_str(byte_idx, &inserted);
        self.cursor += inserted.chars().count();
    }

    // ── Transpose ────────────────────────────────────────────────────

    /// Swap the two chars around the cursor and advance one position.
    /// Matches Emacs `transpose-chars`:
    ///
    /// - cursor at end-of-line: swap the two chars just before the cursor
    ///   (cursor stays).
    /// - cursor strictly inside: swap chars at `cursor - 1` and `cursor`,
    ///   then `cursor += 1`.
    /// - cursor at 0 or buffer too short: no-op.
    pub fn transpose_chars(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let n = chars.len();
        if n < 2 || self.cursor == 0 {
            return;
        }
        let (a, b) = if self.cursor < n {
            (self.cursor - 1, self.cursor)
        } else {
            // cursor == n: swap the last two chars (and leave cursor
            // pinned at end-of-line, matching readline).
            (n - 2, n - 1)
        };
        let mut new_chars = chars;
        new_chars.swap(a, b);
        self.text = new_chars.into_iter().collect();
        if self.cursor < n {
            self.cursor += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str, cursor: usize) -> EditBuffer {
        EditBuffer {
            text: text.to_string(),
            cursor,
            kill_ring: None,
        }
    }

    // ── home / end ───────────────────────────────────────────────────

    #[test]
    fn home_resets_cursor() {
        let mut b = buf("hello world", 7);
        b.home();
        assert_eq!(b.cursor, 0);
    }

    #[test]
    fn end_jumps_to_char_count() {
        let mut b = buf("hello", 0);
        b.end();
        assert_eq!(b.cursor, 5);
    }

    #[test]
    fn end_counts_chars_not_bytes() {
        // "héllo" has 5 chars but 6 bytes (é is 2 bytes).
        let mut b = buf("héllo", 0);
        b.end();
        assert_eq!(b.cursor, 5);
    }

    // ── word jumps ───────────────────────────────────────────────────

    #[test]
    fn word_forward_skips_whitespace_then_word() {
        let mut b = buf("foo bar baz", 1);
        b.move_word_forward();
        assert_eq!(b.cursor, 3, "land at end of `foo`");
        b.move_word_forward();
        assert_eq!(b.cursor, 7, "land at end of `bar`");
    }

    #[test]
    fn word_forward_at_end_is_noop() {
        let mut b = buf("foo", 3);
        b.move_word_forward();
        assert_eq!(b.cursor, 3);
    }

    #[test]
    fn word_back_from_end_skips_into_last_word() {
        let mut b = buf("foo bar baz", 11);
        b.move_word_back();
        assert_eq!(b.cursor, 8, "land at start of `baz`");
    }

    #[test]
    fn word_back_from_zero_is_noop() {
        let mut b = buf("foo", 0);
        b.move_word_back();
        assert_eq!(b.cursor, 0);
    }

    #[test]
    fn word_boundary_uses_word_chars_not_whitespace() {
        // `foo.bar.baz` — under the new rule, dots are non-word.
        let mut b = buf("foo.bar.baz", 11);
        b.move_word_back();
        assert_eq!(b.cursor, 8, "land at start of `baz`, not at 0");
    }

    // ── kill ranges ──────────────────────────────────────────────────

    #[test]
    fn kill_to_end_saves_and_truncates() {
        let mut b = buf("hello world", 5);
        b.kill_to_end();
        assert_eq!(b.text, "hello");
        assert_eq!(b.cursor, 5);
        assert_eq!(b.kill_ring.as_deref(), Some(" world"));
    }

    #[test]
    fn kill_to_start_saves_and_resets_cursor() {
        let mut b = buf("hello world", 6);
        b.kill_to_start();
        assert_eq!(b.text, "world");
        assert_eq!(b.cursor, 0);
        assert_eq!(b.kill_ring.as_deref(), Some("hello "));
    }

    #[test]
    fn kill_to_end_at_end_is_noop() {
        let mut b = buf("hello", 5);
        b.kill_to_end();
        assert_eq!(b.text, "hello");
        assert!(b.kill_ring.is_none());
    }

    // ── kill word ────────────────────────────────────────────────────

    #[test]
    fn kill_word_back_saves_word() {
        let mut b = buf("foo bar baz", 11);
        b.kill_word_back();
        assert_eq!(b.text, "foo bar ");
        assert_eq!(b.cursor, 8);
        assert_eq!(b.kill_ring.as_deref(), Some("baz"));
    }

    #[test]
    fn kill_word_back_new_rule_splits_on_punctuation() {
        // Pre-text-input-ux this killed the whole string; new rule:
        // only `baz` (dots are non-word).
        let mut b = buf("foo.bar.baz", 11);
        b.kill_word_back();
        assert_eq!(b.text, "foo.bar.");
        assert_eq!(b.kill_ring.as_deref(), Some("baz"));
    }

    #[test]
    fn kill_word_forward_kills_next_word() {
        let mut b = buf("foo bar baz", 4);
        b.kill_word_forward();
        assert_eq!(b.text, "foo  baz");
        assert_eq!(b.cursor, 4);
        assert_eq!(b.kill_ring.as_deref(), Some("bar"));
    }

    #[test]
    fn delete_word_backward_delegates_to_kill_word_back() {
        // Public alias kept for the 18 existing callers.
        let mut b = buf("alpha beta", 10);
        b.delete_word_backward();
        assert_eq!(b.text, "alpha ");
        assert_eq!(b.kill_ring.as_deref(), Some("beta"));
    }

    // ── yank ─────────────────────────────────────────────────────────

    #[test]
    fn yank_inserts_kill_ring_at_cursor() {
        let mut b = EditBuffer {
            text: "hello".to_string(),
            cursor: 5,
            kill_ring: Some(" world".to_string()),
        };
        b.yank();
        assert_eq!(b.text, "hello world");
        assert_eq!(b.cursor, 11);
        // Ring is not cleared.
        assert_eq!(b.kill_ring.as_deref(), Some(" world"));
    }

    #[test]
    fn yank_without_ring_is_noop() {
        let mut b = buf("hello", 5);
        b.yank();
        assert_eq!(b.text, "hello");
    }

    #[test]
    fn double_yank_inserts_twice() {
        let mut b = EditBuffer {
            text: String::new(),
            cursor: 0,
            kill_ring: Some("foo".to_string()),
        };
        b.yank();
        b.yank();
        assert_eq!(b.text, "foofoo");
        assert_eq!(b.cursor, 6);
    }

    // ── transpose ────────────────────────────────────────────────────

    #[test]
    fn transpose_inside_swaps_and_advances() {
        let mut b = buf("helol", 4);
        b.transpose_chars();
        assert_eq!(b.text, "hello");
        assert_eq!(b.cursor, 5);
    }

    #[test]
    fn transpose_at_end_swaps_last_two() {
        let mut b = buf("helol", 5);
        b.transpose_chars();
        assert_eq!(b.text, "hello");
        assert_eq!(b.cursor, 5);
    }

    #[test]
    fn transpose_at_zero_is_noop() {
        let mut b = buf("abc", 0);
        b.transpose_chars();
        assert_eq!(b.text, "abc");
    }

    #[test]
    fn transpose_short_buffer_is_noop() {
        let mut b = buf("a", 1);
        b.transpose_chars();
        assert_eq!(b.text, "a");
    }

    // ── multi-byte chars ─────────────────────────────────────────────

    #[test]
    fn kill_to_end_handles_multibyte() {
        let mut b = buf("hé world", 2); // cursor after "hé"
        b.kill_to_end();
        assert_eq!(b.text, "hé");
        assert_eq!(b.kill_ring.as_deref(), Some(" world"));
    }

    #[test]
    fn yank_multibyte_advances_by_char_count() {
        let mut b = EditBuffer {
            text: String::new(),
            cursor: 0,
            kill_ring: Some("café".to_string()),
        };
        b.yank();
        assert_eq!(b.text, "café");
        assert_eq!(b.cursor, 4, "advanced by 4 chars, not 5 bytes");
    }
}
