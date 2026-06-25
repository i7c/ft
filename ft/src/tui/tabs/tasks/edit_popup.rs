//! The shared task-edit popup state — the form both the Tasks tab and the
//! Graph tab open to edit a task's fields in place. Lifted out of
//! `tasks/search.rs` (graph-task-interaction §6) so the Graph tab can host
//! the same popup via a `TaskEdit` modal without duplicating the field
//! definitions, navigation, and constructors.
//!
//! The *commit* wiring (reading the popup, calling `ops::*`, refreshing)
//! is host-specific and stays in each tab; only the state + pure helpers
//! live here.

use chrono::NaiveDate;
use ft_core::task::{Priority, Task};

use crate::tui::widgets::{EditBuffer, FuzzyPicker, VaultFilePickerSource};

/// The popup's editable form. Holds one `EditBuffer` per field plus a
/// focus cursor, an optional error line, the mode (edit vs new), and an
/// optional target picker for the `New` mode's file-selection field.
///
/// Not `Clone`/`Debug`: the `target_picker` holds a `Matcher`. Nothing
/// currently relies on those bounds.
pub(crate) struct EditPopup {
    pub description: EditBuffer,
    pub target: EditBuffer,
    pub due: EditBuffer,
    pub scheduled: EditBuffer,
    pub priority: EditBuffer,
    pub tags: EditBuffer,
    pub recurrence: EditBuffer,
    pub focus: EditField,
    pub error: Option<String>,
    pub mode: PopupMode,
    pub target_picker: Option<FuzzyPicker<VaultFilePickerSource>>,
}

/// What the popup is doing: editing the task at `(path, line)` or
/// creating a fresh one. The target field is only relevant in `New`
/// mode — edits don't move the task to a different file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PopupMode {
    Edit,
    New,
}

/// Validated popup fields ready to be applied to disk: (description,
/// due, scheduled, priority, tags, recurrence). Aliased so the two
/// submit-path methods don't trip the `type_complexity` lint each time
/// they pass the tuple around.
pub(crate) type PopupFields = (
    String,
    Option<NaiveDate>,
    Option<NaiveDate>,
    Option<Priority>,
    Vec<String>,
    Option<String>,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditField {
    Description,
    Target,
    Due,
    Scheduled,
    Priority,
    Tags,
    Recurrence,
}

impl EditField {
    pub fn label(self) -> &'static str {
        match self {
            EditField::Description => "description",
            EditField::Target => "target",
            EditField::Due => "due",
            EditField::Scheduled => "scheduled",
            EditField::Priority => "priority",
            EditField::Tags => "tags",
            EditField::Recurrence => "recurrence",
        }
    }
}

impl EditPopup {
    /// Pre-populate from the selected task so the popup opens with the
    /// current state and the user can edit-in-place.
    pub fn from_task(task: &Task) -> Self {
        Self {
            description: EditBuffer::from(&task.description),
            target: EditBuffer::default(),
            due: EditBuffer::from(&fmt_date(task.due)),
            scheduled: EditBuffer::from(&fmt_date(task.scheduled)),
            priority: EditBuffer::from(priority_text(task.priority)),
            tags: EditBuffer::from(&task.tags.join(" ")),
            recurrence: EditBuffer::from(task.recurrence.as_deref().unwrap_or("")),
            focus: EditField::Description,
            error: None,
            mode: PopupMode::Edit,
            target_picker: None,
        }
    }

    /// Blank "new task" popup. Opened by `Shift+C` from the search view.
    pub fn new_blank() -> Self {
        Self {
            description: EditBuffer::default(),
            target: EditBuffer::default(),
            due: EditBuffer::default(),
            scheduled: EditBuffer::default(),
            priority: EditBuffer::default(),
            tags: EditBuffer::default(),
            recurrence: EditBuffer::default(),
            focus: EditField::Description,
            error: None,
            mode: PopupMode::New,
            target_picker: None,
        }
    }

    /// "New task" popup pre-filled from a parsed quickline. Opened by
    /// `Ctrl+E` so the user can fall through to the full form with their
    /// in-flight quickline state intact.
    pub fn from_quickline(parse: &crate::tui::tabs::tasks::quickline::QuicklineParse) -> Self {
        let target = parse
            .target
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        Self {
            description: EditBuffer::from(&parse.description),
            target: EditBuffer::from(&target),
            due: EditBuffer::from(&fmt_date(parse.due)),
            scheduled: EditBuffer::from(&fmt_date(parse.scheduled)),
            priority: EditBuffer::from(priority_text(parse.priority)),
            tags: EditBuffer::from(&parse.tags.join(" ")),
            recurrence: EditBuffer::from(parse.recurrence.as_deref().unwrap_or("")),
            focus: EditField::Description,
            error: None,
            mode: PopupMode::New,
            target_picker: None,
        }
    }

    /// Ordered field list for the current mode. Edit mode skips the
    /// `target` field because the task already lives in a known file
    /// (moving a task is a separate `m` operation, not part of edit).
    pub fn fields(&self) -> &'static [EditField] {
        match self.mode {
            PopupMode::Edit => &[
                EditField::Description,
                EditField::Due,
                EditField::Scheduled,
                EditField::Priority,
                EditField::Tags,
                EditField::Recurrence,
            ],
            PopupMode::New => &[
                EditField::Description,
                EditField::Target,
                EditField::Due,
                EditField::Scheduled,
                EditField::Priority,
                EditField::Tags,
                EditField::Recurrence,
            ],
        }
    }

    pub fn next_field(&self) -> EditField {
        let fields = self.fields();
        let i = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        fields[(i + 1) % fields.len()]
    }

    pub fn prev_field(&self) -> EditField {
        let fields = self.fields();
        let i = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        fields[(i + fields.len() - 1) % fields.len()]
    }

    pub fn focused_buffer_mut(&mut self) -> &mut EditBuffer {
        match self.focus {
            EditField::Description => &mut self.description,
            EditField::Target => &mut self.target,
            EditField::Due => &mut self.due,
            EditField::Scheduled => &mut self.scheduled,
            EditField::Priority => &mut self.priority,
            EditField::Tags => &mut self.tags,
            EditField::Recurrence => &mut self.recurrence,
        }
    }

    pub fn buffer_for(&self, field: EditField) -> &EditBuffer {
        match field {
            EditField::Description => &self.description,
            EditField::Target => &self.target,
            EditField::Due => &self.due,
            EditField::Scheduled => &self.scheduled,
            EditField::Priority => &self.priority,
            EditField::Tags => &self.tags,
            EditField::Recurrence => &self.recurrence,
        }
    }
}

pub fn fmt_date(d: Option<NaiveDate>) -> String {
    d.map(|x| x.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// Compact relative date label (mirrors the Tasks tab's row formatting).
/// Used by the Graph tab's task `leaf_display` for display parity
/// (graph-task-interaction §D6).
pub fn relative_date(d: NaiveDate, today: NaiveDate) -> String {
    let diff = (d - today).num_days();
    match diff {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        n if (-6..=-2).contains(&n) => format!("{}d ago", -n),
        n if (2..=6).contains(&n) => format!("in {}d", n),
        n if (-13..=-7).contains(&n) => "1w ago".into(),
        n if (7..=13).contains(&n) => "in 1w".into(),
        n if (-20..=-14).contains(&n) => "2w ago".into(),
        n if (14..=20).contains(&n) => "in 2w".into(),
        n if (-27..=-21).contains(&n) => "3w ago".into(),
        n if (21..=27).contains(&n) => "in 3w".into(),
        n if (-30..=-28).contains(&n) => "4w ago".into(),
        n if (28..=30).contains(&n) => "in 4w".into(),
        _ => d.format("%Y-%m-%d").to_string(),
    }
}

pub fn priority_text(p: Option<Priority>) -> &'static str {
    match p {
        None => "",
        Some(Priority::Lowest) => "lowest",
        Some(Priority::Low) => "low",
        Some(Priority::Medium) => "medium",
        Some(Priority::High) => "high",
        Some(Priority::Highest) => "highest",
    }
}
