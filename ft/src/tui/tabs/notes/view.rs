//! Notes tab renderer. The idle body is a keymap-style panel; an opt-in
//! help overlay floats above it on `?`; the open-flow picker and the
//! section-move flow each render their own centered popup over the body.

use std::collections::BTreeSet;

use ft_core::markdown::Heading;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::tab::TabCtx;
use crate::tui::tabs::notes::{
    is_implicitly_selected, ClipboardItem, ComposeRow, CreateState, NewTargetState, NotesState,
    RenameBuffer, SectionMoveState,
};

/// Idle-panel keymap. Each row is `(keys, description)`. Kept identical to
/// the `?` help overlay so users see one canonical list.
const IDLE_KEYS: &[(&str, &str)] = &[
    ("o", "open file / heading"),
    ("m", "move section(s) to another file"),
    ("c", "create note (blank)"),
    ("C", "create note from template"),
    ("?", "show this help"),
    ("Esc", "close overlay"),
];

/// Open-flow picker keymap shown along the bottom while the open-flow
/// picker is on screen. Mirrors the bindings in `mod.rs`.
const OPEN_PICKER_KEYS: &[(&str, &str)] = &[
    ("Enter", "open in $EDITOR"),
    ("Ctrl+O", "open in Obsidian"),
    ("Esc", "back to idle"),
];

/// Footer keymap for step 1/4 of the section-move flow.
const MOVE_STEP_1_KEYS: &[(&str, &str)] = &[("Enter", "use source"), ("Esc", "cancel move")];

/// Footer keymap for step 2/4 (heading multi-select).
const MOVE_STEP_2_KEYS: &[(&str, &str)] = &[
    ("↑/↓", "focus"),
    ("Space", "toggle"),
    ("Enter", "next"),
    ("Esc", "back"),
];

/// Footer keymap for step 3/4 (target picker).
const MOVE_STEP_3_KEYS: &[(&str, &str)] = &[
    ("Enter", "use target"),
    ("Ctrl+N", "new target"),
    ("Esc", "back to selection"),
];

/// Footer keymaps for the new-target sub-flow (plan 009 session 5).
const MOVE_NEW_TEMPLATE_KEYS: &[(&str, &str)] =
    &[("Enter", "use template"), ("Esc", "back to target picker")];
const MOVE_NEW_FOLDER_KEYS: &[(&str, &str)] =
    &[("Enter", "use folder"), ("Esc", "back to template")];
const MOVE_NEW_FILENAME_KEYS: &[(&str, &str)] = &[
    ("Enter", "next"),
    ("Esc", "back to folder"),
    ("Ctrl+W", "delete word"),
];
const MOVE_NEW_VAR_KEYS: &[(&str, &str)] = &[
    ("Enter", "next var / compose"),
    ("Esc", "cancel new target"),
];
const MOVE_NEW_COLLISION_KEYS: &[(&str, &str)] = &[
    ("o", "overwrite"),
    ("u", "use existing"),
    ("c", "cancel"),
    ("←/→", "focus"),
    ("Enter", "commit"),
];

/// Footer keymap for step 4/4 (compose).
const MOVE_STEP_4_KEYS: &[(&str, &str)] = &[
    ("↑/↓", "focus"),
    ("Shift+↑/↓", "reorder"),
    ("←/→", "level"),
    ("r", "rename"),
    ("Enter", "commit"),
    ("Esc", "back"),
];

/// Footer keymap shown while the inline rename buffer is open. Replaces
/// the standard compose keymap so the user only sees keys that actually
/// fire under the buffer.
const MOVE_STEP_4_RENAME_KEYS: &[(&str, &str)] = &[("Enter", "commit rename"), ("Esc", "discard")];

/// Footer keymap for the create-flow template picker.
const CREATE_TEMPLATE_KEYS: &[(&str, &str)] = &[("Enter", "use template"), ("Esc", "cancel")];
/// Footer keymap for the create-flow folder picker.
const CREATE_FOLDER_KEYS: &[(&str, &str)] = &[("Enter", "use folder"), ("Esc", "back")];
/// Footer keymap for the filename prompt.
const CREATE_FILENAME_KEYS: &[(&str, &str)] = &[
    ("Enter", "commit"),
    ("Esc", "back"),
    ("Ctrl+W", "delete word"),
];
/// Footer keymap for var prompts.
const CREATE_VAR_KEYS: &[(&str, &str)] = &[("Enter", "next"), ("Esc", "cancel create")];
/// Footer keymap for the collision prompt.
const CREATE_COLLISION_KEYS: &[(&str, &str)] = &[
    ("o", "overwrite"),
    ("u", "use existing"),
    ("c", "cancel"),
    ("←/→", "focus"),
    ("Enter", "commit"),
];

pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    _ctx: &TabCtx,
    state: &mut NotesState,
    show_help: bool,
) {
    render_idle_body(frame, area);

    match state {
        NotesState::Idle => {
            if show_help {
                render_help_overlay(frame, area);
            }
        }
        NotesState::OpenPicking { picker } => {
            render_picker_popup(
                frame,
                area,
                " open · pick file / heading ",
                picker,
                OPEN_PICKER_KEYS,
                None,
            );
        }
        NotesState::MoveSection(ms) => render_move_overlay(frame, area, ms),
        NotesState::Creating(cs) => render_create_overlay(frame, area, cs),
    }
}

fn render_create_overlay(frame: &mut Frame, area: Rect, cs: &mut CreateState) {
    use crate::tui::tabs::notes::CreateState as CS;
    match cs {
        CS::TemplatePicking { picker } => render_path_picker_popup(
            frame,
            area,
            " create · 1/4 template ",
            picker,
            CREATE_TEMPLATE_KEYS,
        ),
        CS::FolderPicking { template, picker } => {
            let step = step_count(template.as_ref());
            let title = match template {
                Some(t) => format!(" create · 2/{step} folder · {} ", t.rel.display()),
                None => format!(" create · 1/{step} folder · blank "),
            };
            render_path_picker_popup(frame, area, &title, picker, CREATE_FOLDER_KEYS);
        }
        CS::FilenamePrompt {
            template,
            folder,
            buf,
            error,
        } => render_filename_prompt(
            frame,
            area,
            template.as_ref(),
            folder,
            buf,
            error.as_deref(),
        ),
        CS::VarPrompt {
            template,
            folder,
            filename,
            vars_so_far,
            next_idx,
            buf,
        } => render_var_prompt(
            frame,
            area,
            template,
            folder,
            filename,
            vars_so_far,
            *next_idx,
            buf,
        ),
        CS::CollisionPrompt {
            template,
            folder,
            filename,
            vars: _,
            abs_path: _,
            focus,
        } => render_collision_prompt(frame, area, template.as_ref(), folder, filename, *focus),
    }
}

fn step_count(template: Option<&crate::tui::tabs::notes::TemplatePick>) -> usize {
    match template {
        None => 2,
        Some(t) if t.vars_needed.is_empty() => 3,
        Some(_) => 4,
    }
}

fn render_path_picker_popup(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    picker: &mut crate::tui::widgets::FuzzyPicker<crate::tui::widgets::PathListPickerSource>,
    keys: &[(&str, &str)],
) {
    let popup = centered_rect(60, 70, area);
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);
    picker.render(frame, chunks[0]);
    let footer = keymap_line(keys);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), footer]).alignment(Alignment::Center),
        chunks[1],
    );
}

fn render_filename_prompt(
    frame: &mut Frame,
    area: Rect,
    template: Option<&crate::tui::tabs::notes::TemplatePick>,
    folder: &std::path::Path,
    buf: &crate::tui::widgets::EditBuffer,
    error: Option<&str>,
) {
    let popup = centered_rect(60, 30, area);
    frame.render_widget(Clear, popup);
    let folder_label = if folder.as_os_str().is_empty() {
        ".".to_string()
    } else {
        folder.display().to_string()
    };
    let title = match template {
        Some(t) => format!(
            " create · 3/{step} filename · {} → {} ",
            t.rel.display(),
            folder_label,
            step = step_count(Some(t))
        ),
        None => format!(
            " create · 2/{step} filename · {} ",
            folder_label,
            step = step_count(None)
        ),
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let footer_height = if error.is_some() { 3 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(inner);

    let input = Line::from(vec![
        Span::styled(
            "▏ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("filename: ", Style::default().fg(Color::Gray)),
        Span::styled(buf.text.clone(), Style::default().fg(Color::White)),
        Span::styled(
            "█",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]);
    frame.render_widget(Paragraph::new(input), chunks[0]);

    let mut footer_lines: Vec<Line> = Vec::with_capacity(2);
    if let Some(msg) = error {
        footer_lines.push(Line::from(Span::styled(
            msg.to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }
    footer_lines.push(keymap_line(CREATE_FILENAME_KEYS));
    frame.render_widget(
        Paragraph::new(footer_lines).alignment(Alignment::Center),
        chunks[2],
    );
}

#[allow(clippy::too_many_arguments)]
fn render_var_prompt(
    frame: &mut Frame,
    area: Rect,
    template: &crate::tui::tabs::notes::TemplatePick,
    folder: &std::path::Path,
    filename: &str,
    vars_so_far: &std::collections::BTreeMap<String, String>,
    next_idx: usize,
    buf: &crate::tui::widgets::EditBuffer,
) {
    let popup = centered_rect(60, 35, area);
    frame.render_widget(Clear, popup);
    let folder_label = if folder.as_os_str().is_empty() {
        ".".to_string()
    } else {
        folder.display().to_string()
    };
    let total = template.vars_needed.len();
    let cur = next_idx + 1;
    let key_name = template
        .vars_needed
        .get(next_idx)
        .map(|s| s.as_str())
        .unwrap_or("?");
    let title = format!(
        " create · 4/{step} vars · {folder_label}/{filename} · {cur}/{total} ",
        step = step_count(Some(template))
    );
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner);

    let prompt = Line::from(vec![
        Span::styled(
            "▏ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{key_name} = "), Style::default().fg(Color::Gray)),
        Span::styled(buf.text.clone(), Style::default().fg(Color::White)),
        Span::styled(
            "█",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]);
    frame.render_widget(Paragraph::new(prompt), chunks[0]);

    let so_far_label: String = if vars_so_far.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = vars_so_far
            .iter()
            .map(|(k, v)| format!("{k}={v:?}"))
            .collect();
        format!("set: {}", parts.join(", "))
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            so_far_label,
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[1],
    );

    let footer = keymap_line(CREATE_VAR_KEYS);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), footer]).alignment(Alignment::Center),
        chunks[3],
    );
}

fn render_collision_prompt(
    frame: &mut Frame,
    area: Rect,
    _template: Option<&crate::tui::tabs::notes::TemplatePick>,
    folder: &std::path::Path,
    filename: &str,
    focus: crate::tui::tabs::notes::CollisionChoice,
) {
    let rel = if folder.as_os_str().is_empty() {
        filename.to_string()
    } else {
        format!("{}/{}", folder.display(), filename)
    };
    use crate::tui::tabs::notes::CollisionChoice as CC;
    let popup = centered_rect(60, 30, area);
    frame.render_widget(Clear, popup);
    let title = " create · collision ".to_string();
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("target already exists: {rel}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        chunks[0],
    );

    let opt_span = |label: &str, choice: CC| {
        let style = if focus == choice {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        Span::styled(format!(" {label} "), style)
    };
    let opts = Line::from(vec![
        opt_span("[O]verwrite", CC::Overwrite),
        Span::raw("   "),
        opt_span("[U]se existing", CC::UseExisting),
        Span::raw("   "),
        opt_span("[C]ancel", CC::Cancel),
    ]);
    frame.render_widget(Paragraph::new(opts).alignment(Alignment::Center), chunks[2]);

    let footer = keymap_line(CREATE_COLLISION_KEYS);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), footer]).alignment(Alignment::Center),
        chunks[4],
    );
}

fn render_move_overlay(frame: &mut Frame, area: Rect, ms: &mut SectionMoveState) {
    match ms {
        SectionMoveState::SourcePicking { picker } => {
            render_picker_popup(
                frame,
                area,
                " move · 1/4 source ",
                picker,
                MOVE_STEP_1_KEYS,
                None,
            );
        }
        SectionMoveState::HeadingMultiSelect {
            source_rel,
            headings,
            selected,
            focus,
            ..
        } => {
            render_multiselect_popup(
                frame,
                area,
                source_rel.display().to_string(),
                headings,
                selected,
                *focus,
            );
        }
        SectionMoveState::TargetPicking {
            source_rel,
            clipboard,
            picker,
            error,
            ..
        } => {
            let title = format!(
                " move · 3/4 target · {} from {} ",
                clipboard.len(),
                source_rel.display()
            );
            render_picker_popup(
                frame,
                area,
                &title,
                picker,
                MOVE_STEP_3_KEYS,
                error.as_deref(),
            );
        }
        SectionMoveState::Composing {
            target_rel,
            target_is_new,
            clipboard,
            layout,
            focus,
            editing,
            ..
        } => {
            let label = if *target_is_new {
                format!("{} (new)", target_rel.display())
            } else {
                target_rel.display().to_string()
            };
            render_compose_popup(
                frame,
                area,
                label,
                clipboard,
                layout,
                *focus,
                editing.as_ref(),
            );
        }
        SectionMoveState::NewTargetCreating(nts) => render_new_target_overlay(frame, area, nts),
    }
}

fn render_new_target_overlay(frame: &mut Frame, area: Rect, nts: &mut NewTargetState) {
    use std::collections::BTreeMap;
    match nts {
        NewTargetState::TemplatePicking { picker, .. } => render_path_picker_popup(
            frame,
            area,
            " move · new target · template ",
            picker,
            MOVE_NEW_TEMPLATE_KEYS,
        ),
        NewTargetState::FolderPicking {
            template, picker, ..
        } => {
            let title = match template {
                Some(t) => format!(" move · new target · folder · {} ", t.rel.display()),
                None => " move · new target · folder · blank ".to_string(),
            };
            render_path_picker_popup(frame, area, &title, picker, MOVE_NEW_FOLDER_KEYS);
        }
        NewTargetState::FilenamePrompt {
            template,
            folder,
            buf,
            error,
            ..
        } => render_new_target_filename_prompt(
            frame,
            area,
            template.as_ref(),
            folder,
            buf,
            error.as_deref(),
        ),
        NewTargetState::VarPrompt {
            template,
            folder,
            filename,
            vars_so_far,
            next_idx,
            buf,
            ..
        } => render_new_target_var_prompt(
            frame,
            area,
            template,
            folder,
            filename,
            vars_so_far,
            *next_idx,
            buf,
        ),
        NewTargetState::CollisionPrompt {
            template,
            target_rel,
            focus,
            ..
        } => {
            let _ = BTreeMap::<String, String>::new();
            render_new_target_collision_prompt(frame, area, template.as_ref(), target_rel, *focus)
        }
    }
}

fn render_new_target_filename_prompt(
    frame: &mut Frame,
    area: Rect,
    template: Option<&crate::tui::tabs::notes::TemplatePick>,
    folder: &std::path::Path,
    buf: &crate::tui::widgets::EditBuffer,
    error: Option<&str>,
) {
    let popup = centered_rect(60, 30, area);
    frame.render_widget(Clear, popup);
    let folder_label = if folder.as_os_str().is_empty() {
        ".".to_string()
    } else {
        folder.display().to_string()
    };
    let title = match template {
        Some(t) => format!(
            " move · new target · filename · {} → {} ",
            t.rel.display(),
            folder_label
        ),
        None => format!(" move · new target · filename · {} (blank) ", folder_label),
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let footer_height = if error.is_some() { 3 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(inner);

    let input = Line::from(vec![
        Span::styled(
            "▏ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("filename: ", Style::default().fg(Color::Gray)),
        Span::styled(buf.text.clone(), Style::default().fg(Color::White)),
        Span::styled(
            "█",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]);
    frame.render_widget(Paragraph::new(input), chunks[0]);

    let mut footer_lines: Vec<Line> = Vec::with_capacity(2);
    if let Some(msg) = error {
        footer_lines.push(Line::from(Span::styled(
            msg.to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }
    footer_lines.push(keymap_line(MOVE_NEW_FILENAME_KEYS));
    frame.render_widget(
        Paragraph::new(footer_lines).alignment(Alignment::Center),
        chunks[2],
    );
}

#[allow(clippy::too_many_arguments)]
fn render_new_target_var_prompt(
    frame: &mut Frame,
    area: Rect,
    template: &crate::tui::tabs::notes::TemplatePick,
    folder: &std::path::Path,
    filename: &str,
    vars_so_far: &std::collections::BTreeMap<String, String>,
    next_idx: usize,
    buf: &crate::tui::widgets::EditBuffer,
) {
    let popup = centered_rect(60, 35, area);
    frame.render_widget(Clear, popup);
    let folder_label = if folder.as_os_str().is_empty() {
        ".".to_string()
    } else {
        folder.display().to_string()
    };
    let total = template.vars_needed.len();
    let cur = next_idx + 1;
    let key_name = template
        .vars_needed
        .get(next_idx)
        .map(|s| s.as_str())
        .unwrap_or("?");
    let title = format!(" move · new target · vars · {folder_label}/{filename} · {cur}/{total} ");
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner);

    let prompt = Line::from(vec![
        Span::styled(
            "▏ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{key_name} = "), Style::default().fg(Color::Gray)),
        Span::styled(buf.text.clone(), Style::default().fg(Color::White)),
        Span::styled(
            "█",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]);
    frame.render_widget(Paragraph::new(prompt), chunks[0]);

    let so_far_label: String = if vars_so_far.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = vars_so_far
            .iter()
            .map(|(k, v)| format!("{k}={v:?}"))
            .collect();
        format!("set: {}", parts.join(", "))
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            so_far_label,
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[1],
    );

    let footer = keymap_line(MOVE_NEW_VAR_KEYS);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), footer]).alignment(Alignment::Center),
        chunks[3],
    );
}

fn render_new_target_collision_prompt(
    frame: &mut Frame,
    area: Rect,
    _template: Option<&crate::tui::tabs::notes::TemplatePick>,
    target_rel: &std::path::Path,
    focus: crate::tui::tabs::notes::CollisionChoice,
) {
    use crate::tui::tabs::notes::CollisionChoice as CC;
    let popup = centered_rect(60, 30, area);
    frame.render_widget(Clear, popup);
    let title = " move · new target · collision ".to_string();
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("target already exists: {}", target_rel.display()),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        chunks[0],
    );

    let opt_span = |label: &str, choice: CC| {
        let style = if focus == choice {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        Span::styled(format!(" {label} "), style)
    };
    let opts = Line::from(vec![
        opt_span("[O]verwrite", CC::Overwrite),
        Span::raw("   "),
        opt_span("[U]se existing", CC::UseExisting),
        Span::raw("   "),
        opt_span("[C]ancel", CC::Cancel),
    ]);
    frame.render_widget(Paragraph::new(opts).alignment(Alignment::Center), chunks[2]);

    let footer = keymap_line(MOVE_NEW_COLLISION_KEYS);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), footer]).alignment(Alignment::Center),
        chunks[4],
    );
}

fn render_compose_popup(
    frame: &mut Frame,
    area: Rect,
    target_label: String,
    clipboard: &[ClipboardItem],
    layout: &[ComposeRow],
    focus: usize,
    editing: Option<&RenameBuffer>,
) {
    let popup = centered_rect(70, 80, area);
    frame.render_widget(Clear, popup);
    let pending_count = layout
        .iter()
        .filter(|r| matches!(r, ComposeRow::Pending { .. }))
        .count();
    let title = format!(" move · 4/4 compose · {pending_count} pending → {target_label} ");
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    // When the rename buffer is open, reserve one extra line above the
    // footer for the inline edit field. We keep the body the same height
    // and the edit row floats just under it.
    let edit_height: u16 = if editing.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(edit_height),
            Constraint::Length(2),
        ])
        .split(inner);

    let body_area = chunks[0];
    let visible = body_area.height as usize;
    let total = layout.len();
    let scroll = compute_scroll(focus, visible, total);
    let end = (scroll + visible).min(total);

    let mut lines: Vec<Line> = Vec::with_capacity(end.saturating_sub(scroll));
    for i in scroll..end {
        lines.push(render_compose_row(layout, clipboard, focus, i));
    }
    frame.render_widget(Paragraph::new(lines), body_area);

    if let Some(rb) = editing {
        let edit_line = Line::from(vec![
            Span::styled(
                "▏ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("rename → ", Style::default().fg(Color::Yellow)),
            Span::styled(rb.buf.text.clone(), Style::default().fg(Color::White)),
            Span::styled(
                "█",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ]);
        frame.render_widget(Paragraph::new(edit_line), chunks[1]);
    }

    let keys = if editing.is_some() {
        MOVE_STEP_4_RENAME_KEYS
    } else {
        MOVE_STEP_4_KEYS
    };
    let footer = keymap_line(keys);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), footer]).alignment(Alignment::Center),
        chunks[2],
    );
}

fn render_compose_row(
    layout: &[ComposeRow],
    clipboard: &[ClipboardItem],
    focus: usize,
    i: usize,
) -> Line<'static> {
    let row = &layout[i];
    let cursor = if i == focus { "▶ " } else { "  " };
    let (level, text, is_pending, rename) = match row {
        ComposeRow::Anchor { level, text, .. } => (*level, text.clone(), false, None),
        ComposeRow::Pending {
            clip_idx,
            level,
            rename,
        } => (
            *level,
            clipboard[*clip_idx].source_text.clone(),
            true,
            rename.clone(),
        ),
    };
    let indent = "  ".repeat((level as usize).saturating_sub(1));
    let level_tag = format!("H{level}  ");
    let marker = if is_pending { "+ " } else { "· " };
    let row_style = if i == focus {
        Style::default()
            .bg(Color::Rgb(40, 40, 60))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let marker_style = if is_pending {
        row_style.fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(Color::DarkGray)
    };
    let text_style = if is_pending {
        row_style.fg(Color::White)
    } else {
        row_style.fg(Color::DarkGray)
    };
    let mut spans = vec![
        Span::styled(cursor, row_style),
        Span::styled(marker, marker_style),
        Span::styled(indent, row_style),
        Span::styled(level_tag, row_style.fg(Color::DarkGray)),
        Span::styled(text, text_style),
    ];
    if let Some(new_text) = rename {
        spans.push(Span::styled(
            "  → ",
            row_style.fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            new_text,
            row_style.fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn render_picker_popup(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    picker: &mut crate::tui::widgets::FuzzyPicker<crate::tui::widgets::VaultFilePickerSource>,
    keys: &[(&str, &str)],
    error: Option<&str>,
) {
    let popup = centered_rect(60, 70, area);
    frame.render_widget(Clear, popup);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title.to_string())
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let footer_height = if error.is_some() { 3 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(footer_height)])
        .split(inner);

    picker.render(frame, chunks[0]);

    let mut footer_lines: Vec<Line> = Vec::with_capacity(2);
    if let Some(msg) = error {
        footer_lines.push(Line::from(Span::styled(
            msg.to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }
    footer_lines.push(keymap_line(keys));
    frame.render_widget(
        Paragraph::new(footer_lines).alignment(Alignment::Center),
        chunks[1],
    );
}

fn render_multiselect_popup(
    frame: &mut Frame,
    area: Rect,
    source_label: String,
    headings: &[Heading],
    selected: &BTreeSet<usize>,
    focus: usize,
) {
    let popup = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup);
    let title = format!(" move · 2/4 select · {source_label} ");
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);

    let body_area = chunks[0];
    let visible = body_area.height as usize;
    let total = headings.len();
    let scroll = compute_scroll(focus, visible, total);
    let end = (scroll + visible).min(total);

    let mut lines: Vec<Line> = Vec::with_capacity(end.saturating_sub(scroll));
    for i in scroll..end {
        lines.push(render_multiselect_row(headings, selected, focus, i));
    }
    frame.render_widget(Paragraph::new(lines), body_area);

    let footer = keymap_line(MOVE_STEP_2_KEYS);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), footer]).alignment(Alignment::Center),
        chunks[1],
    );
}

fn render_multiselect_row(
    headings: &[Heading],
    selected: &BTreeSet<usize>,
    focus: usize,
    i: usize,
) -> Line<'static> {
    let h = &headings[i];
    let explicit = selected.contains(&h.line);
    let implicit = !explicit && is_implicitly_selected(headings, i, selected);
    let marker = if explicit {
        "■"
    } else if implicit {
        "▣"
    } else {
        "□"
    };
    let cursor = if i == focus { "▶ " } else { "  " };
    let indent = "  ".repeat((h.level as usize).saturating_sub(1));
    let level_tag = format!("H{}  ", h.level);
    let row_style = if i == focus {
        Style::default()
            .bg(Color::Rgb(40, 40, 60))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let marker_style = if explicit {
        row_style.fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if implicit {
        row_style.fg(Color::DarkGray)
    } else {
        row_style.fg(Color::White)
    };
    let text_style = if implicit {
        row_style.fg(Color::DarkGray)
    } else {
        row_style.fg(Color::White)
    };
    Line::from(vec![
        Span::styled(cursor, row_style),
        Span::styled(format!("{marker} "), marker_style),
        Span::styled(indent, row_style),
        Span::styled(level_tag, row_style.fg(Color::DarkGray)),
        Span::styled(h.text.clone(), text_style),
    ])
}

fn compute_scroll(focus: usize, visible: usize, total: usize) -> usize {
    if total == 0 || visible == 0 || focus < visible {
        return 0;
    }
    if focus >= total {
        return total.saturating_sub(visible);
    }
    focus + 1 - visible
}

fn keymap_line(keys: &[(&str, &str)]) -> Line<'static> {
    Line::from(
        keys.iter()
            .flat_map(|(k, d)| {
                vec![
                    Span::styled(
                        format!(" {k} "),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("{d}  "), Style::default().fg(Color::Gray)),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

fn render_idle_body(frame: &mut Frame, area: Rect) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" notes ")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let mut lines: Vec<Line> = Vec::with_capacity(IDLE_KEYS.len() + 3);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Notes — Obsidian-flavoured editing",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (key, desc) in IDLE_KEYS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<6}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(*desc, Style::default().fg(Color::White)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(50, 50, area);
    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::with_capacity(IDLE_KEYS.len() + 4);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Notes keybindings",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (key, desc) in IDLE_KEYS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<8}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(*desc, Style::default().fg(Color::White)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  press ? or Esc to close",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" notes · help ")
        .style(Style::default().bg(Color::Black));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
