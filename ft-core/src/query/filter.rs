use chrono::NaiveDate;

use crate::task::{Priority, Status, Task};

/// Programmatic filter assembled from CLI flags or compiled DSL.
///
/// Empty / `None` fields are ignored. When multiple fields are set, all must
/// match — i.e. the filter is conjunctive.
#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub statuses: Vec<Status>,
    pub priorities: Vec<Priority>,
    /// Tags the task must have (each entry is a separate `and` clause).
    pub tags: Vec<String>,
    /// Substring(s) the task's `source_file` path must contain.
    pub paths: Vec<String>,
    pub due_before: Option<NaiveDate>,
    pub due_after: Option<NaiveDate>,
    pub scheduled_before: Option<NaiveDate>,
    pub scheduled_after: Option<NaiveDate>,
    /// `Some(true)` requires a due date; `Some(false)` requires no due date.
    pub has_due: Option<bool>,
}

impl Filter {
    pub fn matches(&self, task: &Task) -> bool {
        if !self.statuses.is_empty() && !self.statuses.contains(&task.status) {
            return false;
        }
        if !self.priorities.is_empty() {
            match task.priority {
                Some(p) if self.priorities.contains(&p) => {}
                _ => return false,
            }
        }
        for tag in &self.tags {
            let needle = tag.trim_start_matches('#');
            if !task.tags.iter().any(|t| t == needle) {
                return false;
            }
        }
        for fragment in &self.paths {
            let path_str = task.source_file.to_string_lossy();
            if !path_str.contains(fragment.as_str()) {
                return false;
            }
        }
        if let Some(cutoff) = self.due_before {
            match task.due {
                Some(d) if d < cutoff => {}
                _ => return false,
            }
        }
        if let Some(cutoff) = self.due_after {
            match task.due {
                Some(d) if d > cutoff => {}
                _ => return false,
            }
        }
        if let Some(cutoff) = self.scheduled_before {
            match task.scheduled {
                Some(d) if d < cutoff => {}
                _ => return false,
            }
        }
        if let Some(cutoff) = self.scheduled_after {
            match task.scheduled {
                Some(d) if d > cutoff => {}
                _ => return false,
            }
        }
        if let Some(needs_due) = self.has_due {
            if needs_due != task.due.is_some() {
                return false;
            }
        }
        true
    }

    pub fn apply<'a>(&self, tasks: &'a [Task]) -> Vec<&'a Task> {
        tasks.iter().filter(|t| self.matches(t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn task(desc: &str) -> Task {
        Task {
            description: desc.into(),
            source_file: PathBuf::from("notes/test.md"),
            source_line: 1,
            ..Default::default()
        }
    }

    #[test]
    fn empty_filter_matches_all() {
        let t = task("a");
        assert!(Filter::default().matches(&t));
    }

    #[test]
    fn status_filter() {
        let mut t = task("a");
        t.status = Status::Done;
        let f = Filter {
            statuses: vec![Status::Open],
            ..Default::default()
        };
        assert!(!f.matches(&t));
        let f = Filter {
            statuses: vec![Status::Done, Status::Cancelled],
            ..Default::default()
        };
        assert!(f.matches(&t));
    }

    #[test]
    fn priority_filter_requires_some() {
        let mut t = task("a");
        let f = Filter {
            priorities: vec![Priority::High],
            ..Default::default()
        };
        // task has no priority — does not match
        assert!(!f.matches(&t));
        t.priority = Some(Priority::High);
        assert!(f.matches(&t));
    }

    #[test]
    fn tag_filter_strips_hash() {
        let mut t = task("do #work");
        t.tags = vec!["work".into()];
        let f = Filter {
            tags: vec!["#work".into()],
            ..Default::default()
        };
        assert!(f.matches(&t));
        let f = Filter {
            tags: vec!["work".into()],
            ..Default::default()
        };
        assert!(f.matches(&t));
        let f = Filter {
            tags: vec!["nope".into()],
            ..Default::default()
        };
        assert!(!f.matches(&t));
    }

    #[test]
    fn path_filter_substring() {
        let t = task("a"); // source_file = notes/test.md
        let f = Filter {
            paths: vec!["notes/".into()],
            ..Default::default()
        };
        assert!(f.matches(&t));
        let f = Filter {
            paths: vec!["projects/".into()],
            ..Default::default()
        };
        assert!(!f.matches(&t));
    }

    #[test]
    fn due_before_after() {
        let mut t = task("a");
        t.due = Some(NaiveDate::from_ymd_opt(2026, 5, 10).unwrap());
        let f = Filter {
            due_before: Some(NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()),
            ..Default::default()
        };
        assert!(f.matches(&t));
        let f = Filter {
            due_after: Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()),
            ..Default::default()
        };
        assert!(!f.matches(&t));
    }

    #[test]
    fn has_due_flag() {
        let t = task("no due");
        let f = Filter {
            has_due: Some(true),
            ..Default::default()
        };
        assert!(!f.matches(&t));
        let f = Filter {
            has_due: Some(false),
            ..Default::default()
        };
        assert!(f.matches(&t));
    }

    #[test]
    fn conjunctive_combination() {
        let mut t = task("do #work");
        t.tags = vec!["work".into()];
        t.priority = Some(Priority::High);
        let f = Filter {
            tags: vec!["work".into()],
            priorities: vec![Priority::High],
            ..Default::default()
        };
        assert!(f.matches(&t));
        let f = Filter {
            tags: vec!["work".into()],
            priorities: vec![Priority::Low],
            ..Default::default()
        };
        assert!(!f.matches(&t));
    }
}
