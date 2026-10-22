//! Task colors on the Tasks tab: a `#color/<name>` tag on a task line
//! colors its row and, through inheritance, its subtree.

use ft_core::task::Task;
use ratatui::style::Color;

/// One of the six terminal (ANSI) colors a task can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskColor {
    Red,
    Yellow,
    Green,
    Blue,
    Magenta,
    Cyan,
}

impl TaskColor {
    pub const ALL: [TaskColor; 6] = [
        TaskColor::Red,
        TaskColor::Yellow,
        TaskColor::Green,
        TaskColor::Blue,
        TaskColor::Magenta,
        TaskColor::Cyan,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TaskColor::Red => "red",
            TaskColor::Yellow => "yellow",
            TaskColor::Green => "green",
            TaskColor::Blue => "blue",
            TaskColor::Magenta => "magenta",
            TaskColor::Cyan => "cyan",
        }
    }

    /// The bare tag (no `#`), e.g. `color/red`.
    pub fn tag(self) -> String {
        format!("color/{}", self.name())
    }

    pub fn color(self) -> Color {
        match self {
            TaskColor::Red => Color::Red,
            TaskColor::Yellow => Color::Yellow,
            TaskColor::Green => Color::Green,
            TaskColor::Blue => Color::Blue,
            TaskColor::Magenta => Color::Magenta,
            TaskColor::Cyan => Color::Cyan,
        }
    }

    /// The color a bare tag names (`color/blue`), if it is one of ours.
    fn from_tag(tag: &str) -> Option<TaskColor> {
        let name = tag.strip_prefix("color/")?;
        TaskColor::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The task's own color: its first known `#color/<name>` tag.
    pub fn of(task: &Task) -> Option<TaskColor> {
        task.tags.iter().find_map(|t| TaskColor::from_tag(t))
    }

    /// Every bare color tag, the switchable list for retagging.
    pub fn all_tags() -> Vec<String> {
        TaskColor::ALL.iter().map(|c| c.tag()).collect()
    }
}

/// `description` without its known `#color/<name>` words, for display.
pub fn strip_tags(description: &str) -> String {
    if !description.contains("#color/") {
        return description.to_string();
    }
    description
        .split_whitespace()
        .filter(|w| w.strip_prefix('#').and_then(TaskColor::from_tag).is_none())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(tags: &[&str]) -> Task {
        Task {
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn first_known_color_tag_wins() {
        assert_eq!(
            TaskColor::of(&task(&["work", "color/teal", "color/blue", "color/red"])),
            Some(TaskColor::Blue)
        );
        assert_eq!(TaskColor::of(&task(&["work"])), None);
    }

    #[test]
    fn strip_hides_known_color_tags_only() {
        assert_eq!(strip_tags("Ship Q4 #color/blue #work"), "Ship Q4 #work");
        assert_eq!(strip_tags("x #color/teal"), "x #color/teal");
        assert_eq!(strip_tags("plain  text"), "plain  text");
    }

    #[test]
    fn tags_round_trip() {
        for c in TaskColor::ALL {
            assert_eq!(TaskColor::from_tag(&c.tag()), Some(c));
        }
        assert_eq!(TaskColor::all_tags().len(), 6);
    }
}
