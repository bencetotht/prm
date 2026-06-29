use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::state::{AddProjectField, AppState, FocusPane, Modal};
use crate::fs::agents::AgentsContent;
use crate::git::{GitHistory, GitRelease};
use crate::meta;
use crate::ui::layout::{centered_rect, split_main, split_right_column};
use crate::ui::theme;
use crate::ui::widgets::pane_block;

const TODO_HIGHLIGHT_SYMBOL: &str = "▌ ";
const TODO_ELLIPSIS: &str = "...";
const INPUT_TRUNCATION_MARKER: &str = "<";

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let (panes, footer) = split_main(frame.area());
    let (right_top, right_bottom) = split_right_column(panes[2]);

    render_projects(frame, app, panes[0]);
    render_todos(frame, app, panes[1]);
    render_agents(frame, app, right_top);
    render_git_history(frame, app, right_bottom);
    render_footer(frame, app, footer);

    if app.show_help {
        render_help_overlay(frame, app);
    }

    if let Some(modal) = app.modal.clone() {
        render_modal(frame, modal);
    }
}

fn render_projects(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let title = if app.filter_input.is_empty() {
        "[1] Projects"
    } else {
        "[1] Projects (filtered)"
    };
    let block = pane_block(title, app.focus == FocusPane::Projects);

    if app.projects.is_empty() {
        let text = if app.filter_input.trim().is_empty() {
            vec![
                Line::styled("No projects found", theme::header_style()),
                Line::from("Use `prm add .` or press `a` in this pane."),
            ]
        } else {
            vec![
                Line::styled("No projects match the filter", theme::header_style()),
                Line::from("Edit the filter below, or press Esc to close it."),
            ]
        };
        let widget = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
        return;
    }

    let items = app
        .projects
        .iter()
        .map(|project| {
            let marker = if project.archived { "[A]" } else { "   " };
            let git_status = app.project_git_status(&project.path);
            let release = app.project_git_release(&project.path);
            let active_todo_count = app.project_active_todo_count(project.id);
            ListItem::new(Line::from(vec![
                Span::raw(format!("{marker} ")),
                Span::styled(
                    project_active_todo_count_prefix(active_todo_count),
                    theme::count_style(),
                ),
                Span::styled(
                    format!("[{}]", git_status.short_label()),
                    theme::git_status_style(&git_status),
                ),
                Span::raw(format!(" {}", project.name)),
                Span::styled(project_release_suffix(release), theme::muted_style()),
            ]))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_item_style())
        .highlight_symbol("▌ ");

    let mut state = ListState::default();
    state.select(Some(app.selected_project));
    frame.render_stateful_widget(list, area, &mut state);
}

fn project_active_todo_count_prefix(count: usize) -> String {
    let label = if count > 9 {
        "9+".to_string()
    } else {
        count.to_string()
    };
    format!("{label:>2} ")
}

fn project_release_suffix(release: GitRelease) -> String {
    match release {
        GitRelease::Tagged { tag, .. } => format!("  {tag}"),
        GitRelease::NoTags => String::new(),
        GitRelease::NoCommits => "  no-commits".to_string(),
        GitRelease::NotGit => String::new(),
        GitRelease::Error(_) => "  tag-error".to_string(),
    }
}

fn render_todos(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let is_markdown = app
        .selected_project()
        .map(|p| p.todo_source == "markdown")
        .unwrap_or(false);
    let title = if is_markdown {
        "[2] Todos (TODO.md)"
    } else {
        "[2] Todos"
    };
    let block = pane_block(title, app.focus == FocusPane::Todos);
    let list_width = block.inner(area).width as usize;

    if app.todos.is_empty() {
        let text = vec![
            Line::styled("No todos", theme::header_style()),
            Line::from("Press `n` to add one."),
        ];
        let widget = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
        return;
    }

    let items = app
        .todos
        .iter()
        .map(|todo| {
            let check = if todo.done { "[x]" } else { "[ ]" };
            let prefix = format!("{check} ");
            let title_width = list_width
                .saturating_sub(TODO_HIGHLIGHT_SYMBOL.width())
                .saturating_sub(prefix.width());
            let title = truncate_with_ellipsis(&todo.title, title_width);
            let style = if todo.done {
                theme::done_todo_style()
            } else {
                Style::default()
            };
            ListItem::new(Line::styled(format!("{prefix}{title}"), style))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_item_style())
        .highlight_symbol(TODO_HIGHLIGHT_SYMBOL);

    let mut state = ListState::default();
    state.select(Some(app.selected_todo));
    frame.render_stateful_widget(list, area, &mut state);
}

fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
    if text.width() <= max_width {
        return text.to_string();
    }

    if max_width <= TODO_ELLIPSIS.width() {
        return ".".repeat(max_width);
    }

    let content_width = max_width - TODO_ELLIPSIS.width();
    let mut width = 0;
    let mut truncated = String::new();

    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > content_width {
            break;
        }

        width += ch_width;
        truncated.push(ch);
    }

    truncated.push_str(TODO_ELLIPSIS);
    truncated
}

fn render_agents(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let title = scrolled_title("[3] AGENTS.md", app.agents_scroll);
    let block = pane_block(&title, app.focus == FocusPane::Agents);

    let body = match app.current_agents_content() {
        AgentsContent::Missing => "No AGENTS.md found".to_string(),
        AgentsContent::Loaded(content) => content,
        AgentsContent::Error(err) => format!("Read error: {err}"),
    };

    let paragraph = Paragraph::new(body)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.agents_scroll, 0));

    frame.render_widget(paragraph, area);
}

fn render_git_history(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let title = scrolled_title("[4] Git history", app.git_history_scroll);
    let block = pane_block(&title, app.focus == FocusPane::GitHistory);
    let mut body = match app.current_git_history() {
        GitHistory::NotGit => vec![Line::from("Not a git repository")],
        GitHistory::Empty => vec![Line::from("No commits yet")],
        GitHistory::Error(err) => vec![Line::from(format!("Read error: {err}"))],
        GitHistory::Lines(lines) => lines.into_iter().map(Line::from).collect(),
    };

    let release_line = match app.current_git_release() {
        GitRelease::Tagged {
            tag,
            commits_ahead: 0,
        } => {
            format!("Release: {tag} (HEAD at tag)")
        }
        GitRelease::Tagged { tag, commits_ahead } => {
            format!("Release: {tag} (+{commits_ahead} commits)")
        }
        GitRelease::NoTags => "Release: no tags in history".to_string(),
        GitRelease::NoCommits => "Release: no commits yet".to_string(),
        GitRelease::NotGit => "Release: n/a (not a git repository)".to_string(),
        GitRelease::Error(err) => format!("Release: read error ({err})"),
    };

    body.insert(0, Line::from(""));
    body.insert(0, Line::styled(release_line, theme::header_style()));

    let paragraph = Paragraph::new(body)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.git_history_scroll, 0));
    frame.render_widget(paragraph, area);
}

fn scrolled_title(title: &str, scroll: u16) -> String {
    if scroll == 0 {
        title.to_string()
    } else {
        format!("{title} +{scroll}")
    }
}

fn render_footer(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    if app.filter_mode {
        render_input_field(
            frame,
            area,
            "Filter projects - Enter apply, Esc cancel",
            &app.filter_input,
            app.filter_cursor,
            true,
            "",
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let status = Paragraph::new(Line::from(vec![
        Span::styled("Status ", theme::muted_style()),
        Span::styled(app.status.as_str(), theme::status_style()),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(status, rows[0]);

    let shortcuts = Paragraph::new(shortcut_line(app))
        .style(theme::help_style())
        .wrap(Wrap { trim: true });
    frame.render_widget(shortcuts, rows[1]);

    let global = Paragraph::new(Line::from(vec![
        key_span("Tab"),
        Span::raw(" panes  "),
        key_span("?"),
        Span::raw(" help  "),
        key_span("/"),
        Span::raw(" filter  "),
        key_span("f"),
        Span::raw(" fetch  "),
        key_span("Q"),
        Span::raw(" quit"),
    ]))
    .style(theme::help_style())
    .wrap(Wrap { trim: true });
    frame.render_widget(global, rows[2]);
}

fn shortcut_line(app: &AppState) -> Line<'static> {
    match app.focus {
        FocusPane::Projects => Line::from(vec![
            Span::styled("Projects ", theme::muted_style()),
            key_span("a"),
            Span::raw(" add  "),
            key_span("r"),
            Span::raw(" rename  "),
            key_span("x"),
            Span::raw(" archive  "),
            key_span("d"),
            Span::raw(" delete  "),
            key_span("A"),
            Span::raw(" archived  "),
            key_span("m"),
            Span::raw(" todo source  "),
            key_span("g"),
            Span::raw(" lazygit  "),
            key_span("t"),
            Span::raw(" terminal"),
        ]),
        FocusPane::Todos => Line::from(vec![
            Span::styled("Todos ", theme::muted_style()),
            key_span("n"),
            Span::raw(" add  "),
            key_span("e/Enter"),
            Span::raw(" edit  "),
            key_span("Space"),
            Span::raw(" done  "),
            key_span("dd"),
            Span::raw(" delete  "),
            key_span("J/K PgUp/PgDn"),
            Span::raw(" move active"),
        ]),
        FocusPane::Agents => Line::from(vec![
            Span::styled("AGENTS.md ", theme::muted_style()),
            key_span("j/k"),
            Span::raw(" scroll  "),
            key_span("Up/Down"),
            Span::raw(" scroll  "),
            key_span("mouse wheel"),
            Span::raw(" scroll"),
        ]),
        FocusPane::GitHistory => Line::from(vec![
            Span::styled("Git history ", theme::muted_style()),
            key_span("j/k"),
            Span::raw(" scroll  "),
            key_span("Up/Down"),
            Span::raw(" scroll  "),
            key_span("g"),
            Span::raw(" lazygit"),
        ]),
    }
}

fn key_span(key: &'static str) -> Span<'static> {
    Span::styled(key, theme::shortcut_style())
}

fn render_input_field(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    value: &str,
    cursor: usize,
    active: bool,
    hint: &str,
) {
    let border_style = if active {
        theme::active_field_border_style()
    } else {
        theme::inactive_field_border_style()
    };
    let block = Block::bordered()
        .title(label.to_string())
        .title_style(theme::pane_title_style(active))
        .border_style(border_style);
    let inner = block.inner(area);
    let edit_width = inner.width.saturating_sub(1) as usize;
    let (display_value, cursor_width) = input_view(value, cursor, edit_width);

    let content = if hint.is_empty() {
        vec![Line::styled(
            display_value,
            theme::input_value_style(active),
        )]
    } else {
        vec![
            Line::styled(display_value, theme::input_value_style(active)),
            Line::styled(hint.to_string(), theme::muted_style()),
        ]
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);

    if active && inner.height > 0 && inner.width > 0 {
        let cursor_x = inner
            .x
            .saturating_add(cursor_width.min(usize::from(inner.width.saturating_sub(1))) as u16);
        frame.set_cursor_position(Position {
            x: cursor_x,
            y: inner.y,
        });
    }
}

fn input_view(value: &str, cursor: usize, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }

    let cursor = clamp_cursor(value, cursor);
    if value.width() <= max_width {
        return (value.to_string(), value[..cursor].width());
    }

    let before = &value[..cursor];
    let after = &value[cursor..];
    let marker_width = INPUT_TRUNCATION_MARKER.width();
    let mut before_chars = Vec::new();
    let mut before_width = 0;
    let before_limit = max_width.saturating_sub(marker_width);

    for ch in before.chars().rev() {
        let ch_width = ch.width().unwrap_or(0);
        if before_width + ch_width > before_limit {
            break;
        }
        before_width += ch_width;
        before_chars.push(ch);
    }
    before_chars.reverse();

    let truncated_before = before.width() > before_width;
    let mut display = String::new();
    if truncated_before && marker_width <= max_width {
        display.push_str(INPUT_TRUNCATION_MARKER);
    }
    for ch in before_chars {
        display.push(ch);
    }

    let cursor_width = display.width();
    let mut used_width = cursor_width;
    for ch in after.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if used_width + ch_width > max_width {
            break;
        }
        used_width += ch_width;
        display.push(ch);
    }

    (display, cursor_width)
}

fn clamp_cursor(value: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(value.len());
    while cursor > 0 && !value.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn render_help_overlay(frame: &mut Frame<'_>, app: &AppState) {
    let area = centered_rect(86, 84, frame.area());
    frame.render_widget(Clear, area);

    let metadata = format!(
        "Version: {} | GitHub: {}",
        meta::version(),
        meta::GITHUB_URL
    );

    let lines = vec![
        Line::styled("PRM Keymap", theme::header_style()),
        Line::from(""),
        Line::styled("Navigation", theme::header_style()),
        help_row("1/2/3/4", "jump to a pane by number"),
        help_row("Tab/Shift-Tab", "switch pane focus"),
        help_row("Left/Right or h/l", "switch pane focus"),
        help_row(
            "Up/Down or j/k",
            "move selection or scroll focused text panes",
        ),
        help_row("Mouse", "click focuses/selects; wheel scrolls under cursor"),
        Line::from(""),
        Line::styled("Refresh", theme::header_style()),
        help_row(
            "auto",
            "database checks every 2s; git refresh runs every 60s",
        ),
        help_row("f", "fetch immediately: database, git, and pane caches"),
        Line::from(""),
        Line::styled("Global", theme::header_style()),
        help_row("/", "filter projects by name/path"),
        help_row("g", "open lazygit for selected project"),
        help_row(
            "t",
            "open a new project terminal tab/window via tmux or cmux",
        ),
        help_row("?", "toggle this help dialog"),
        help_row("Q", "quit prm"),
        Line::from(""),
        Line::styled("Projects pane", theme::header_style()),
        help_row("a", "add project with path and optional name"),
        help_row("r", "rename selected project"),
        help_row("x", "archive or unarchive selected project"),
        help_row("d", "delete selected project with confirmation"),
        help_row("A", "toggle showing archived projects"),
        help_row("m", "toggle todo storage: database or TODO.md file"),
        help_row("tags", "dim project suffix shows latest reachable git tag"),
        help_row(
            "git",
            "CHG changed; PUSH waiting; COMMIT local-only; OK synced",
        ),
        Line::from(""),
        Line::styled("Todos pane", theme::header_style()),
        help_row("n", "add todo"),
        help_row("e/Enter", "edit selected todo"),
        help_row("Space", "toggle done"),
        help_row("dd", "delete selected todo"),
        help_row("J/K, PgUp/PgDn, Shift+Up/Down", "move active todo"),
        Line::from(""),
        Line::styled("Input fields", theme::header_style()),
        help_row("Left/Right", "move cursor"),
        help_row("Home/End", "jump to start or end"),
        help_row("Backspace/Delete", "remove text around cursor"),
        help_row("Enter/Esc", "submit or cancel"),
        Line::from(""),
        Line::styled("Text panes", theme::header_style()),
        help_row(
            "[3]/[4]",
            "AGENTS.md and Git history support keyboard and mouse scrolling",
        ),
        help_row("[4]", "shows nearest release tag and commit distance"),
        Line::from(""),
        Line::styled("Build Info", theme::header_style()),
        Line::from(metadata),
        Line::from(meta::copyright_line()),
        help_row(
            "Esc/q/?",
            "close help; scroll with j/k, arrows, PgUp/PgDn, wheel",
        ),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::bordered()
                .title(scrolled_title("Help", app.help_scroll))
                .title_style(theme::header_style())
                .border_style(theme::focus_border_style()),
        )
        .style(theme::help_style())
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll, 0));

    frame.render_widget(widget, area);
}

fn help_row(key: &'static str, action: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:<18}"), theme::shortcut_style()),
        Span::raw(action),
    ])
}

fn render_modal(frame: &mut Frame<'_>, modal: Modal) {
    match modal {
        Modal::Input(input) => {
            let area = centered_rect(62, 30, frame.area());
            frame.render_widget(Clear, area);

            let block = Block::bordered()
                .title("Input")
                .title_style(theme::header_style())
                .border_style(theme::focus_border_style());
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            frame.render_widget(
                Paragraph::new(input.title).style(theme::header_style()),
                rows[0],
            );
            render_input_field(
                frame,
                rows[2],
                &input.prompt,
                &input.value,
                input.cursor,
                true,
                "",
            );
            render_modal_hint(frame, rows[4], "Enter submit | Esc cancel");
        }
        Modal::AddProject(add) => {
            let area = centered_rect(72, 40, frame.area());
            frame.render_widget(Clear, area);

            let block = Block::bordered()
                .title("Project")
                .title_style(theme::header_style())
                .border_style(theme::focus_border_style());
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            frame.render_widget(
                Paragraph::new("Add project").style(theme::header_style()),
                rows[0],
            );
            render_input_field(
                frame,
                rows[2],
                "Path",
                &add.path,
                add.path_cursor,
                add.active_field == AddProjectField::Path,
                "",
            );
            render_input_field(
                frame,
                rows[4],
                "Name (optional)",
                &add.name,
                add.name_cursor,
                add.active_field == AddProjectField::Name,
                "",
            );
            render_modal_hint(frame, rows[6], "Tab field | Enter submit | Esc cancel");
        }
        Modal::Confirm(confirm) => {
            let area = centered_rect(60, 26, frame.area());
            frame.render_widget(Clear, area);

            let lines = vec![
                Line::styled(confirm.title, theme::danger_style()),
                Line::from(""),
                Line::from(confirm.message),
                Line::from(""),
                Line::from(vec![
                    key_span("Enter/Y"),
                    Span::raw(" confirm  "),
                    key_span("N/Esc"),
                    Span::raw(" cancel"),
                ]),
            ];

            let widget = Paragraph::new(lines)
                .block(
                    Block::bordered()
                        .title("Confirm")
                        .border_style(theme::focus_border_style()),
                )
                .wrap(Wrap { trim: false });
            frame.render_widget(widget, area);
        }
    }
}

fn render_modal_hint(frame: &mut Frame<'_>, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text.to_string())
            .style(theme::muted_style())
            .wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::{project_active_todo_count_prefix, truncate_with_ellipsis};

    #[test]
    fn project_active_todo_count_prefix_caps_after_single_digits() {
        assert_eq!(project_active_todo_count_prefix(0), " 0 ");
        assert_eq!(project_active_todo_count_prefix(9), " 9 ");
        assert_eq!(project_active_todo_count_prefix(10), "9+ ");
    }

    #[test]
    fn truncate_with_ellipsis_keeps_short_text() {
        assert_eq!(truncate_with_ellipsis("short todo", 10), "short todo");
    }

    #[test]
    fn truncate_with_ellipsis_marks_cut_text() {
        assert_eq!(truncate_with_ellipsis("long todo title", 10), "long to...");
    }

    #[test]
    fn truncate_with_ellipsis_handles_tiny_widths() {
        assert_eq!(truncate_with_ellipsis("long", 0), "");
        assert_eq!(truncate_with_ellipsis("long", 1), ".");
        assert_eq!(truncate_with_ellipsis("long", 2), "..");
        assert_eq!(truncate_with_ellipsis("long", 3), "...");
    }

    #[test]
    fn truncate_with_ellipsis_respects_wide_char_width() {
        assert_eq!(truncate_with_ellipsis("ab你好cd", 6), "ab...");
    }
}
