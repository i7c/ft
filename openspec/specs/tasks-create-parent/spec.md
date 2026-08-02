# tasks-create-parent Specification

## Purpose
`ft tasks create` gains a `--parent <SELECTOR>` flag that creates the new
task as an indented subtask of a uniquely resolved parent task, reusing the
existing `ops::Position::Subtask` machinery. The parent is selected by the
standard task-selector forms (`id`, `<file>:<line>`, fuzzy substring) and
must match exactly one task, otherwise the command errors. Created by
change tasks-create-parent.

## Requirements

### Requirement: `--parent` flag on `ft tasks create`

The `ft tasks create` command SHALL accept a `--parent <SELECTOR>` flag
taking a task selector string. The selector SHALL be resolved with the same
rules as the other selector-based `ft tasks` commands: a `<path>:<line>`
form (`file:line`, relative vault path, 1-indexed line, suffix matching
allowed), a bare task id, or a fuzzy substring (case-insensitive match
against description or path, restricted to non-Done tasks). When an
id-shaped selector matches no task by id, the system SHALL fall back to
fuzzy matching (id→fuzzy fallback).

#### Scenario: Parent by `file:line`
- **WHEN** the user runs `ft tasks create "Sub" --parent inbox.md:5` and a
  task exists at `inbox.md:5`
- **THEN** the new task is created as an indented subtask of that task in
  `inbox.md`

#### Scenario: Parent by task id
- **WHEN** the user runs `ft tasks create "Sub" --parent abc123` and a task
  with id `abc123` exists
- **THEN** the new task is created as a subtask of that task

#### Scenario: Parent by fuzzy substring
- **WHEN** the user runs `ft tasks create "Sub" --parent "buy milk"` and
  exactly one open task matches "buy milk" in its description or path
- **THEN** the new task is created as a subtask of that task

#### Scenario: Id-shaped selector falls back to fuzzy
- **WHEN** the user runs `ft tasks create "Sub" --parent build` and no task
  has id `build` but exactly one task description contains "build"
- **THEN** the new task is created as a subtask of that task

### Requirement: unique parent or error

The parent selector SHALL resolve to exactly one task. Zero matches SHALL
produce an error; more than one match SHALL produce an error listing the
candidate tasks (file, line, description) so the user can disambiguate.
No interactive picker is used.

#### Scenario: No matching task errors
- **WHEN** the user runs `ft tasks create "Sub" --parent nope` and no task
  matches
- **THEN** the command fails with an error naming the selector, and no file
  is modified

#### Scenario: Multiple matches error with candidates
- **WHEN** the user runs `ft tasks create "Sub" --parent milk` and more than
  one task matches "milk"
- **THEN** the command fails with an error listing the matching tasks
  (at least up to five, with a count of the remainder) and no file is
  modified

### Requirement: subtask placement in the parent's file

When `--parent` resolves, the new task SHALL be written to the parent's
file as an indented subtask at the end of the parent's indented block,
using the existing `Position::Subtask` placement: indentation matching the
parent's first existing child, or the parent's indentation plus two spaces
when the parent has no children yet. The daily-note default and any
default-section logic are bypassed (the parent's file is the target).

#### Scenario: Parent with existing children
- **WHEN** the user creates a task under a parent that already has an
  indented child at four spaces
- **THEN** the new task is written after the parent's last child, indented
  four spaces

#### Scenario: Parent without children
- **WHEN** the user creates a task under a parent that has no children
- **THEN** the new task is written immediately after the parent line,
  indented two spaces deeper than the parent

### Requirement: conflict with placement and file flags

`--parent` SHALL conflict with `--file`, `--under-heading`, `--at-line`,
and `--append`: passing any of them together with `--parent` SHALL fail
argument parsing before any vault work.

#### Scenario: `--parent` plus `--file` errors
- **WHEN** the user runs `ft tasks create "Sub" --parent inbox.md:5 --file other.md`
- **THEN** the command fails with a clap conflict error and no file is
  modified

#### Scenario: `--parent` plus `--at-line` errors
- **WHEN** the user runs `ft tasks create "Sub" --parent inbox.md:5 --at-line 3`
- **THEN** the command fails with a clap conflict error and no file is
  modified

### Requirement: duplicate check applies

The duplicate check SHALL apply to subtask creation exactly as to top-level
creation: an existing task in the parent's file with the same description,
due, scheduled, and start dates blocks insertion unless `--force` is passed.

#### Scenario: Duplicate subtask blocked without `--force`
- **WHEN** the user creates a subtask whose description and dates match an
  existing task in the parent's file
- **THEN** the command fails with the duplicate error naming the existing
  `file:line`, and no line is inserted

#### Scenario: `--force` inserts despite duplicate
- **WHEN** the user repeats the same creation with `--force`
- **THEN** the subtask is inserted despite the duplicate

### Requirement: done tasks remain selectable via id and `file:line`

A done task SHALL be a valid parent when selected by id or `file:line`
(fuzzy matching already excludes done tasks).

#### Scenario: Done parent by `file:line`
- **WHEN** the user runs `ft tasks create "Sub" --parent inbox.md:5` and
  the task at `inbox.md:5` is done
- **THEN** the new task is created as a subtask of the done task
