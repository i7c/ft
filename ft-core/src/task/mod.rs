use std::path::PathBuf;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

pub mod emoji;
pub mod format;
pub mod hierarchy;
pub mod ops;
pub mod recurrence;
pub mod resolve;

pub use hierarchy::TaskKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Status {
    #[default]
    Open,
    Done,
    InProgress,
    Cancelled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "Open",
            Status::Done => "Done",
            Status::InProgress => "InProgress",
            Status::Cancelled => "Cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Highest,
    High,
    Medium,
    Low,
    Lowest,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Priority::Highest => "Highest",
            Priority::High => "High",
            Priority::Medium => "Medium",
            Priority::Low => "Low",
            Priority::Lowest => "Lowest",
        }
    }

    pub fn emoji(self) -> &'static str {
        match self {
            Priority::Highest => "🔺",
            Priority::High => "⏫",
            Priority::Medium => "🔼",
            Priority::Low => "🔽",
            Priority::Lowest => "⏬",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub description: String,
    pub status: Status,
    pub priority: Option<Priority>,
    /// Hashtags extracted from the description (e.g. `#work`). The tags remain
    /// in `description` as well; this field is a convenience index.
    pub tags: Vec<String>,
    pub created: Option<NaiveDate>,
    /// 🛫 start date — earliest date to begin working on the task.
    pub start: Option<NaiveDate>,
    /// ⏳ scheduled date — when the task is scheduled to be worked on.
    pub scheduled: Option<NaiveDate>,
    pub due: Option<NaiveDate>,
    pub done: Option<NaiveDate>,
    pub cancelled: Option<NaiveDate>,
    /// Recurrence rule preserved verbatim (e.g. `"every month on the 18th"`).
    pub recurrence: Option<String>,
    pub id: Option<String>,
    pub depends_on: Vec<String>,
    /// Reserved: on-completion action preserved verbatim (not yet parsed).
    pub on_completion: Option<String>,
    /// Obsidian block identifier (the part after `^`).
    pub block_link: Option<String>,
    /// Unknown emoji fields preserved verbatim so no data is lost on rewrite.
    pub raw_trailing: Option<String>,
    pub source_file: PathBuf,
    /// 1-indexed line number within `source_file`.
    pub source_line: usize,
    /// Leading-whitespace byte count (used for hierarchy detection).
    pub indent_level: usize,
    /// `source_line` of the nearest ancestor task with smaller `indent_level`.
    pub parent: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_as_str_exhaustive() {
        assert_eq!(Status::Open.as_str(), "Open");
        assert_eq!(Status::Done.as_str(), "Done");
        assert_eq!(Status::InProgress.as_str(), "InProgress");
        assert_eq!(Status::Cancelled.as_str(), "Cancelled");
    }

    #[test]
    fn priority_as_str_exhaustive() {
        assert_eq!(Priority::Highest.as_str(), "Highest");
        assert_eq!(Priority::High.as_str(), "High");
        assert_eq!(Priority::Medium.as_str(), "Medium");
        assert_eq!(Priority::Low.as_str(), "Low");
        assert_eq!(Priority::Lowest.as_str(), "Lowest");
    }
}
