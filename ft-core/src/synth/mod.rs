//! Synthesis support: callout grammar for protected sections,
//! scaffold planner/applier for `ft synth`, verifier for
//! `ft synth verify`, and pin repairer for `ft synth repair`.
//!
//! A synth note is a regular `.md` file marked `ft.synth.enabled: true` in YAML
//! frontmatter. It contains one or more **protected sections** —
//! callout blocks of the form
//!
//! ```text
//! > [!ft-source] "<vault-rel-path>" L<a>-<b> @<sha7> #<hash6>
//! > <paragraph line 1>
//! > <paragraph line 2>
//! ```
//!
//! Each protected section is one verbatim source paragraph plus enough
//! provenance to verify it remained byte-identical to the pinned git
//! blob: commit SHA prefix, inclusive 1-indexed line range, and a
//! blake3 content-hash prefix. Between callouts, the user writes
//! arbitrary markdown freely.

pub mod accrete;
pub mod callout;
pub mod citations;
pub mod repair;
pub mod reslice;
pub mod scaffold;
pub mod verify;
