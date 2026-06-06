use crate::task::{Priority, Task};

/// Parse a sort-key name (used by CLI `--sort` flag parsing).
pub fn parse_sort_key(s: &str) -> Result<SortKey, String> {
    match s.to_ascii_lowercase().as_str() {
        "due" => Ok(SortKey::Due),
        "scheduled" => Ok(SortKey::Scheduled),
        "priority" => Ok(SortKey::Priority),
        "path" => Ok(SortKey::Path),
        "description" => Ok(SortKey::Description),
        "status" => Ok(SortKey::Status),
        other => Err(format!("unknown sort key `{other}`")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Due,
    Scheduled,
    Priority,
    Path,
    Description,
    Status,
}

/// Default sort order: due date asc (None last), priority desc (Highest
/// first, None last), then path asc.
pub fn default_sort(tasks: &mut [&Task]) {
    tasks.sort_by(|a, b| {
        cmp_due_asc(a, b)
            .then_with(|| cmp_priority_desc(a, b))
            .then_with(|| a.source_file.cmp(&b.source_file))
            .then_with(|| a.source_line.cmp(&b.source_line))
    });
}

pub fn sort_by_keys(tasks: &mut [&Task], keys: &[(SortKey, SortOrder)]) {
    if keys.is_empty() {
        default_sort(tasks);
        return;
    }
    tasks.sort_by(|a, b| {
        for (key, order) in keys {
            let mut ord = compare_key(a, b, *key);
            if *order == SortOrder::Desc {
                ord = ord.reverse();
            }
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
}

fn compare_key(a: &Task, b: &Task, key: SortKey) -> std::cmp::Ordering {
    match key {
        SortKey::Due => cmp_optional(a.due, b.due),
        SortKey::Scheduled => cmp_optional(a.scheduled, b.scheduled),
        SortKey::Priority => priority_rank(a.priority).cmp(&priority_rank(b.priority)),
        SortKey::Path => a.source_file.cmp(&b.source_file),
        SortKey::Description => a.description.cmp(&b.description),
        SortKey::Status => format!("{:?}", a.status).cmp(&format!("{:?}", b.status)),
    }
}

/// `None` sorts after `Some` (so missing-due tasks land at the bottom by
/// default).
fn cmp_optional<T: Ord>(a: Option<T>, b: Option<T>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn cmp_due_asc(a: &Task, b: &Task) -> std::cmp::Ordering {
    cmp_optional(a.due, b.due)
}

fn cmp_priority_desc(a: &Task, b: &Task) -> std::cmp::Ordering {
    // Higher priority sorts first → reverse rank so Highest=0 is "smallest".
    priority_rank(a.priority).cmp(&priority_rank(b.priority))
}

/// Rank where lower = higher priority. `None` sorts last.
fn priority_rank(p: Option<Priority>) -> u8 {
    match p {
        Some(Priority::Highest) => 0,
        Some(Priority::High) => 1,
        Some(Priority::Medium) => 2,
        Some(Priority::Low) => 3,
        Some(Priority::Lowest) => 4,
        None => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Status;
    use chrono::NaiveDate;
    use std::path::PathBuf;

    fn task(
        desc: &str,
        due: Option<(i32, u32, u32)>,
        priority: Option<Priority>,
        path: &str,
    ) -> Task {
        Task {
            description: desc.into(),
            status: Status::Open,
            priority,
            tags: Vec::new(),
            created: None,
            start: None,
            scheduled: None,
            due: due.map(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).unwrap()),
            done: None,
            cancelled: None,
            recurrence: None,
            id: None,
            depends_on: Vec::new(),
            on_completion: None,
            block_link: None,
            raw_trailing: None,
            source_file: PathBuf::from(path),
            source_line: 1,
            indent_level: 0,
            parent: None,
        }
    }

    #[test]
    fn default_sort_due_asc_then_priority_desc() {
        let a = task("a", Some((2026, 5, 20)), None, "z.md");
        let b = task("b", Some((2026, 5, 10)), Some(Priority::Low), "z.md");
        let c = task("c", Some((2026, 5, 10)), Some(Priority::Highest), "z.md");
        let d = task("d", None, Some(Priority::High), "z.md");

        let mut refs: Vec<&Task> = vec![&a, &b, &c, &d];
        default_sort(&mut refs);

        let order: Vec<_> = refs.iter().map(|t| t.description.as_str()).collect();
        assert_eq!(order, vec!["c", "b", "a", "d"]);
    }

    #[test]
    fn explicit_keys_override_default() {
        let a = task("zeta", None, None, "a.md");
        let b = task("alpha", None, None, "z.md");
        let mut refs: Vec<&Task> = vec![&a, &b];
        sort_by_keys(&mut refs, &[(SortKey::Description, SortOrder::Asc)]);
        assert_eq!(refs[0].description, "alpha");
    }
}
