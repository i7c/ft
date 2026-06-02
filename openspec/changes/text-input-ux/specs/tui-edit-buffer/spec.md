## MODIFIED Requirements

### Requirement: `EditBuffer` supports readline-style editing operations

The `EditBuffer` widget SHALL support these operations: `move_line_start`, `move_line_end`, `move_char_back`, `move_char_forward`, `move_word_back`, `move_word_forward`, `kill_to_end`, `kill_to_start`, `kill_word_back`, `kill_word_forward`, `yank`, `transpose_chars`, `delete_char_back`, `delete_char_forward`. Word boundaries SHALL be defined as maximal runs of `[A-Za-z0-9_]`.

#### Scenario: Line-start jump
- **WHEN** the buffer contains `hello world` with the cursor at position 7
- **THEN** `move_line_start` sets the cursor to position 0

#### Scenario: Word-forward jump
- **WHEN** the buffer contains `foo bar baz` with the cursor at position 1
- **THEN** `move_word_forward` moves the cursor to position 3 (end of `foo`), and a second `move_word_forward` moves it to position 7 (end of `bar`)

#### Scenario: Word-back jump from end
- **WHEN** the buffer contains `foo bar baz` with the cursor at position 11
- **THEN** `move_word_back` moves the cursor to position 8 (start of `baz`)

#### Scenario: Kill to end saves to kill ring
- **WHEN** the buffer contains `hello world` with the cursor at position 5
- **THEN** `kill_to_end` deletes ` world`, leaves the buffer as `hello`, and stores ` world` in the kill ring

#### Scenario: Yank inserts kill ring at cursor
- **WHEN** the kill ring contains ` world` and the buffer is `hello` with the cursor at position 5
- **THEN** `yank` inserts ` world` and the buffer becomes `hello world` with the cursor at position 11

#### Scenario: Transpose swaps chars at and before cursor
- **WHEN** the buffer contains `helol` with the cursor at position 4
- **THEN** `transpose_chars` swaps the chars at positions 3 and 2, producing `hello`

### Requirement: `EditBuffer` holds a one-slot kill ring

The `EditBuffer` SHALL hold at most one killed text region at a time. Each new kill operation SHALL replace the previous kill ring contents. The `yank` operation SHALL insert the kill ring at the cursor without clearing it (multiple yanks insert multiple copies).

#### Scenario: Successive kills replace
- **WHEN** the user presses `Ctrl+K` to kill ` world`, then types text, then `Ctrl+W` to kill a word
- **THEN** the kill ring contains the most recent killed word, not ` world`

#### Scenario: Yank does not clear the ring
- **WHEN** the kill ring contains `foo` and the user yanks twice
- **THEN** the buffer contains `foofoo` at the original cursor's neighborhood

### Requirement: `EditBuffer` optionally hosts a `CompletionProvider`

The `EditBuffer` SHALL optionally hold a `Box<dyn CompletionProvider>`. When set, input mutations matching the provider's `trigger_on()` set SHALL invoke `provider.complete(&ctx)` and SHALL open a `CompletionPopup` rendering the returned items.

#### Scenario: Buffer without provider behaves as before
- **WHEN** an `EditBuffer` has no completion provider set
- **THEN** input events behave identically to the pre-change baseline; no popup ever opens

#### Scenario: Buffer with provider queries on matching input
- **WHEN** an `EditBuffer` has a provider whose `trigger_on()` includes printable characters
- **THEN** typing a printable character invokes `provider.complete(&ctx)` and opens the popup if items are returned

#### Scenario: Provider returns empty list
- **WHEN** the provider returns an empty `Vec<CompletionItem>` for the current context
- **THEN** the popup (if open) closes; no popup opens if it was closed
