//! Editor handoff — pure argv construction for the four
//! [`EditorStrategy`] variants.
//!
//! Used by [`crate::tui::app::App::service_request`] when servicing an
//! [`crate::tui::tab::AppRequest::OpenInEditor`]. Splitting argv
//! construction out of the App lets us test every strategy's shape
//! without spinning up a real tmux server or terminal.
//!
//! See plan 011 for the design rationale (default `tmux-popup`, the
//! `$TMUX`-fallback rule, why we use `--` to separate tmux's flags
//! from the inner command).

use std::path::Path;

use ft_core::config::EditorStrategy;

/// A program to launch and its argv tail. Built by
/// [`build_invocation`] from `(strategy, editor, path, line, popup
/// geometry)`; consumed by [`App::service_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorInvocation {
    pub program: String,
    pub args: Vec<String>,
}

/// Resolve `editor` (the `$EDITOR` / `$VISUAL` string the user has
/// set) to the inner-command argv: the binary plus any extra args the
/// user prefixed (e.g. `EDITOR="code -w"` → `["code", "-w"]`), with
/// `+N` and the path appended.
///
/// Whitespace splitting matches today's behavior (`split_whitespace`).
/// Falls back to `["vi"]` when the editor string is empty — the same
/// guard `spawn_editor` used before plan 011.
fn editor_argv(editor: &str, path: &Path, line: usize) -> Vec<String> {
    let mut parts: Vec<String> = editor.split_whitespace().map(str::to_string).collect();
    if parts.is_empty() {
        parts.push("vi".into());
    }
    parts.push(format!("+{line}"));
    parts.push(path.to_string_lossy().into_owned());
    parts
}

/// Build the launch invocation for a single editor open under
/// `strategy`. Pure — no I/O, no env reads beyond what `editor`
/// already encodes; tests pin `editor` and the strategy and assert on
/// the resulting `EditorInvocation`.
///
/// `popup_width` / `popup_height` are only used when
/// `strategy = TmuxPopup`. Other strategies ignore them.
pub fn build_invocation(
    strategy: EditorStrategy,
    editor: &str,
    path: &Path,
    line: usize,
    popup_width: &str,
    popup_height: &str,
) -> EditorInvocation {
    let inner = editor_argv(editor, path, line);
    match strategy {
        EditorStrategy::Suspend => {
            // First element becomes the program; the rest are args.
            // `editor_argv` guarantees at least one element.
            let mut iter = inner.into_iter();
            let program = iter.next().expect("editor_argv always returns ≥1 element");
            EditorInvocation {
                program,
                args: iter.collect(),
            }
        }
        EditorStrategy::TmuxPopup => {
            let mut args: Vec<String> = vec![
                "display-popup".into(),
                "-E".into(),
                "-w".into(),
                popup_width.to_string(),
                "-h".into(),
                popup_height.to_string(),
                "--".into(),
            ];
            args.extend(inner);
            EditorInvocation {
                program: "tmux".into(),
                args,
            }
        }
        EditorStrategy::TmuxWindow => {
            let mut args: Vec<String> = vec!["new-window".into(), "--".into()];
            args.extend(inner);
            EditorInvocation {
                program: "tmux".into(),
                args,
            }
        }
        EditorStrategy::TmuxSplit => {
            let mut args: Vec<String> = vec!["split-window".into(), "--".into()];
            args.extend(inner);
            EditorInvocation {
                program: "tmux".into(),
                args,
            }
        }
    }
}

/// Quote a single argv element for `sh -c '<…>'` use. Wraps the value
/// in single quotes and escapes any internal single quotes with the
/// `'\''` close-escape-reopen pattern. Safe for arbitrary strings —
/// no shell metacharacters are interpreted inside single quotes.
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Join an argv into a single shell-safe command string. Each element
/// is `shell_quote`d and the results are space-separated.
fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|s| shell_quote(s))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Unique signal name for the `tmux wait-for` handshake used by the
/// `TmuxWindow` / `TmuxSplit` strategies. Combines the PID and the
/// current wall-clock nanos so concurrent ft instances + sequential
/// opens within the same instance never collide.
///
/// tmux signal names share a namespace with the tmux server, not with
/// ft — but the `ft-editor-` prefix plus pid+nanos makes practical
/// collisions vanishingly unlikely.
pub fn unique_signal_name() -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("ft-editor-{pid}-{nanos}")
}

/// Build the launch invocation for `TmuxWindow` / `TmuxSplit` with a
/// `tmux wait-for` handshake appended so ft can block on the
/// editor's exit.
///
/// The tmux subcommand is given `--` then `sh -c '<editor argv>; tmux
/// wait-for -S <signal>'` so the inner shell runs the editor, then
/// signals tmux when the editor exits. After spawning, the caller
/// runs `tmux wait-for <signal>` to block.
///
/// Panics if `strategy` is not `TmuxWindow` or `TmuxSplit`. The popup
/// strategy uses `display-popup -E`, which has its own implicit
/// blocking — no wait-for needed. The suspend strategy needs no
/// handshake at all.
pub fn build_wait_for_invocation(
    strategy: EditorStrategy,
    editor: &str,
    path: &Path,
    line: usize,
    signal: &str,
) -> EditorInvocation {
    let subcommand = match strategy {
        EditorStrategy::TmuxWindow => "new-window",
        EditorStrategy::TmuxSplit => "split-window",
        EditorStrategy::TmuxPopup | EditorStrategy::Suspend => {
            panic!("build_wait_for_invocation called with non-wait strategy: {strategy:?}")
        }
    };
    let inner_argv = editor_argv(editor, path, line);
    let shell_cmd = format!("{}; tmux wait-for -S {}", shell_join(&inner_argv), signal);
    EditorInvocation {
        program: "tmux".into(),
        args: vec![
            subcommand.into(),
            "--".into(),
            "sh".into(),
            "-c".into(),
            shell_cmd,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    // ── Suspend ─────────────────────────────────────────────────────────────

    #[test]
    fn suspend_simple_editor() {
        let inv = build_invocation(
            EditorStrategy::Suspend,
            "nvim",
            &p("/tmp/x.md"),
            42,
            "90%",
            "90%",
        );
        assert_eq!(
            inv,
            EditorInvocation {
                program: "nvim".into(),
                args: vec!["+42".into(), "/tmp/x.md".into()],
            }
        );
    }

    #[test]
    fn suspend_editor_with_extra_args() {
        // EDITOR="code -w" splits into ("code", ["-w"]) — extra args
        // appear *before* "+N" and the path.
        let inv = build_invocation(
            EditorStrategy::Suspend,
            "code -w",
            &p("/tmp/x.md"),
            1,
            "90%",
            "90%",
        );
        assert_eq!(
            inv,
            EditorInvocation {
                program: "code".into(),
                args: vec!["-w".into(), "+1".into(), "/tmp/x.md".into()],
            }
        );
    }

    #[test]
    fn suspend_empty_editor_falls_back_to_vi() {
        let inv = build_invocation(
            EditorStrategy::Suspend,
            "",
            &p("/tmp/x.md"),
            1,
            "90%",
            "90%",
        );
        assert_eq!(inv.program, "vi");
        assert_eq!(inv.args, vec!["+1".to_string(), "/tmp/x.md".to_string()]);
    }

    // ── TmuxPopup ──────────────────────────────────────────────────────────

    #[test]
    fn tmux_popup_default_geometry() {
        let inv = build_invocation(
            EditorStrategy::TmuxPopup,
            "nvim",
            &p("/tmp/x.md"),
            1,
            "90%",
            "90%",
        );
        assert_eq!(
            inv,
            EditorInvocation {
                program: "tmux".into(),
                args: vec![
                    "display-popup".into(),
                    "-E".into(),
                    "-w".into(),
                    "90%".into(),
                    "-h".into(),
                    "90%".into(),
                    "--".into(),
                    "nvim".into(),
                    "+1".into(),
                    "/tmp/x.md".into(),
                ],
            }
        );
    }

    #[test]
    fn tmux_popup_custom_geometry_propagates() {
        let inv = build_invocation(
            EditorStrategy::TmuxPopup,
            "nvim",
            &p("/tmp/x.md"),
            1,
            "80",
            "50%",
        );
        // `-w 80 -h 50%` — verbatim, no validation. tmux is the
        // authoritative parser for its own geometry syntax.
        let win = inv.args.iter().position(|a| a == "-w").unwrap();
        assert_eq!(inv.args[win + 1], "80");
        let hin = inv.args.iter().position(|a| a == "-h").unwrap();
        assert_eq!(inv.args[hin + 1], "50%");
    }

    #[test]
    fn tmux_popup_threads_editor_extra_args() {
        let inv = build_invocation(
            EditorStrategy::TmuxPopup,
            "nvim --clean",
            &p("/tmp/x.md"),
            5,
            "90%",
            "90%",
        );
        // After `--`, the inner argv is the full editor tokenization
        // followed by `+N` and path.
        let dd = inv.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(
            &inv.args[dd + 1..],
            &[
                "nvim".to_string(),
                "--clean".to_string(),
                "+5".to_string(),
                "/tmp/x.md".to_string(),
            ]
        );
    }

    // ── TmuxWindow / TmuxSplit ─────────────────────────────────────────────

    #[test]
    fn tmux_window_argv_shape() {
        let inv = build_invocation(
            EditorStrategy::TmuxWindow,
            "nvim",
            &p("/tmp/x.md"),
            7,
            "90%",
            "90%",
        );
        assert_eq!(
            inv,
            EditorInvocation {
                program: "tmux".into(),
                args: vec![
                    "new-window".into(),
                    "--".into(),
                    "nvim".into(),
                    "+7".into(),
                    "/tmp/x.md".into(),
                ],
            }
        );
    }

    #[test]
    fn tmux_split_argv_shape() {
        let inv = build_invocation(
            EditorStrategy::TmuxSplit,
            "nvim",
            &p("/tmp/x.md"),
            9,
            "90%",
            "90%",
        );
        assert_eq!(
            inv,
            EditorInvocation {
                program: "tmux".into(),
                args: vec![
                    "split-window".into(),
                    "--".into(),
                    "nvim".into(),
                    "+9".into(),
                    "/tmp/x.md".into(),
                ],
            }
        );
    }

    // ── path quoting ───────────────────────────────────────────────────────

    #[test]
    fn path_with_space_passes_as_one_argv_element() {
        // Spaces in the path must NOT cause the path to be split into
        // multiple argv elements — that's why we don't shell-interpolate.
        let inv = build_invocation(
            EditorStrategy::TmuxPopup,
            "nvim",
            &p("/tmp/has space/file.md"),
            1,
            "90%",
            "90%",
        );
        assert_eq!(inv.args.last().unwrap(), "/tmp/has space/file.md");
    }

    #[test]
    fn path_starting_with_dash_safe_under_tmux_double_dash() {
        // The `--` before the editor argv shields paths starting with
        // `-` from being parsed as tmux flags.
        let inv = build_invocation(
            EditorStrategy::TmuxWindow,
            "nvim",
            &p("-weird-path.md"),
            1,
            "90%",
            "90%",
        );
        let dd = inv.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(&inv.args[dd + 1..], &["nvim", "+1", "-weird-path.md"]);
    }

    // ── shell_quote ────────────────────────────────────────────────────────

    #[test]
    fn shell_quote_plain_string() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn shell_quote_string_with_space() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
    }

    #[test]
    fn shell_quote_string_with_single_quote() {
        // The close-escape-reopen pattern: ' → '\''
        assert_eq!(shell_quote("Bob's"), r"'Bob'\''s'");
    }

    #[test]
    fn shell_quote_metacharacters_are_inert_inside_single_quotes() {
        // No expansion happens inside single quotes — `$`, `` ` ``,
        // `\`, `*`, etc. all pass through.
        assert_eq!(
            shell_quote("$HOME/`evil` * \\test"),
            "'$HOME/`evil` * \\test'"
        );
    }

    #[test]
    fn shell_join_round_trip_argv() {
        let argv = vec![
            "nvim".to_string(),
            "+1".to_string(),
            "/tmp/x.md".to_string(),
        ];
        assert_eq!(shell_join(&argv), "'nvim' '+1' '/tmp/x.md'");
    }

    // ── unique_signal_name ────────────────────────────────────────────────

    #[test]
    fn unique_signal_name_is_well_formed_and_distinct() {
        let a = unique_signal_name();
        // sleep-free: nanos resolution is plenty to differentiate
        // sequential calls on any modern system, but we sleep a tiny
        // amount just to make the assertion robust across clocks.
        std::thread::sleep(std::time::Duration::from_nanos(1));
        let b = unique_signal_name();
        assert!(a.starts_with("ft-editor-"));
        assert!(b.starts_with("ft-editor-"));
        assert_ne!(a, b, "sequential calls must produce distinct signals");
    }

    // ── build_wait_for_invocation ────────────────────────────────────────

    #[test]
    fn wait_for_window_wraps_in_shell_with_handshake() {
        let inv = build_wait_for_invocation(
            EditorStrategy::TmuxWindow,
            "nvim",
            &p("/tmp/x.md"),
            42,
            "ft-editor-test-1",
        );
        assert_eq!(inv.program, "tmux");
        assert_eq!(
            inv.args,
            vec![
                "new-window".to_string(),
                "--".into(),
                "sh".into(),
                "-c".into(),
                "'nvim' '+42' '/tmp/x.md'; tmux wait-for -S ft-editor-test-1".into(),
            ]
        );
    }

    #[test]
    fn wait_for_split_uses_split_window_subcommand() {
        let inv =
            build_wait_for_invocation(EditorStrategy::TmuxSplit, "nvim", &p("/tmp/x.md"), 1, "sig");
        assert_eq!(inv.args[0], "split-window");
    }

    #[test]
    fn wait_for_path_with_quote_escaped_correctly() {
        let inv = build_wait_for_invocation(
            EditorStrategy::TmuxWindow,
            "nvim",
            &p("/tmp/Bob's notes.md"),
            1,
            "sig",
        );
        // Final argv element is the shell command; the path with the
        // embedded apostrophe must be quoted via the close-escape-reopen
        // pattern so `sh -c` parses it as one token.
        let sh_cmd = inv.args.last().unwrap();
        assert!(
            sh_cmd.contains(r"'/tmp/Bob'\''s notes.md'"),
            "shell cmd should escape internal quote, got: {sh_cmd}"
        );
    }

    #[test]
    fn wait_for_editor_with_extra_args_quoted_separately() {
        let inv = build_wait_for_invocation(
            EditorStrategy::TmuxWindow,
            "nvim --clean",
            &p("/tmp/x.md"),
            1,
            "sig",
        );
        let sh_cmd = inv.args.last().unwrap();
        assert_eq!(
            sh_cmd,
            "'nvim' '--clean' '+1' '/tmp/x.md'; tmux wait-for -S sig"
        );
    }

    #[test]
    #[should_panic(expected = "non-wait strategy")]
    fn wait_for_panics_on_suspend() {
        let _ =
            build_wait_for_invocation(EditorStrategy::Suspend, "nvim", &p("/tmp/x.md"), 1, "sig");
    }

    #[test]
    #[should_panic(expected = "non-wait strategy")]
    fn wait_for_panics_on_popup() {
        let _ =
            build_wait_for_invocation(EditorStrategy::TmuxPopup, "nvim", &p("/tmp/x.md"), 1, "sig");
    }
}
