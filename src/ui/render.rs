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
        let text = vec![
            Line::from("No projects found"),
            Line::from("Use: prm add ."),
            Line::from("or press a in this pane"),
        ];
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
                    theme::muted_style(),
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
        let text = vec![Line::from("No todos"), Line::from("Press n to add one")];
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
    let block = pane_block("[3] AGENTS.md", app.focus == FocusPane::Agents);

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
    let block = pane_block("[4] Git history", app.focus == FocusPane::GitHistory);
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

fn render_footer(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let content = if app.filter_mode {
        format!("Filter: {}", app.filter_input)
    } else {
        format!(
            "{} | arrows/hjkl move | Tab panes | f fetch | g lazygit | t terminal | a/r/x/d project | n/e/space/dd todo | ? help | Q quit",
            app.status
        )
    };

    let paragraph = Paragraph::new(content)
        .style(theme::status_style())
        .wrap(Wrap { trim: true })
        .block(Block::default());
    frame.render_widget(paragraph, area);
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
        Line::from("1/2/3/4 jump to a pane by number"),
        Line::from("Tab/Shift-Tab or Left/Right switch pane focus"),
        Line::from("Up/Down or j/k move list selection or scroll focused text panes"),
        Line::from("Mouse: left click focuses/selects; wheel scrolls the pane under cursor"),
        Line::from(""),
        Line::styled("Refresh", theme::header_style()),
        Line::from("Database auto-refresh checks external changes every 2 seconds"),
        Line::from("Git status/history auto-refresh runs every 60 seconds"),
        Line::from("Press f to fetch immediately (database + git + pane caches)"),
        Line::from(""),
        Line::styled("Global", theme::header_style()),
        Line::from("/ filter projects by name/path (Enter apply, Esc cancel)"),
        Line::from("g opens lazygit for selected project (tmux popup when available)"),
        Line::from("t opens a new tmux terminal window at selected project path"),
        Line::from("? toggles this help dialog"),
        Line::from("Q quits prm"),
        Line::from(""),
        Line::styled("Projects pane", theme::header_style()),
        Line::from("a open add-project modal (path + optional name)"),
        Line::from("r rename selected project"),
        Line::from("x archive/unarchive selected project"),
        Line::from("d delete selected project (confirmation modal)"),
        Line::from("A toggle showing archived projects"),
        Line::from("m toggle todo storage (database / TODO.md file)"),
        Line::from("Dim suffix in project rows shows the latest reachable git tag"),
        Line::from(
            "Git badge legend: CHG changed, PUSH waiting to push, COMMIT local-only, OK synced",
        ),
        Line::from(""),
        Line::styled("Todos pane", theme::header_style()),
        Line::from("n add todo, e/Enter edit todo, Space toggle done"),
        Line::from("dd delete selected todo"),
        Line::from("J/K reorder selected todo"),
        Line::from(""),
        Line::styled("Text panes", theme::header_style()),
        Line::from("[3] AGENTS.md and [4] Git history support keyboard + mouse scrolling"),
        Line::from("[4] also shows nearest release tag and commit distance from that tag"),
        Line::from(""),
        Line::styled("Build Info", theme::header_style()),
        Line::from(metadata),
        Line::from(meta::copyright_line()),
        Line::from("Scroll: j/k, arrows, PgUp/PgDn, mouse wheel | Close: Esc, q, or ?"),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::bordered()
                .title("Help")
                .title_style(theme::header_style())
                .border_style(theme::focus_border_style()),
        )
        .style(theme::help_style())
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll, 0));

    frame.render_widget(widget, area);
}

fn render_modal(frame: &mut Frame<'_>, modal: Modal) {
    match modal {
        Modal::Input(input) => {
            let area = centered_rect(60, 30, frame.area());
            frame.render_widget(Clear, area);

            let lines = vec![
                Line::styled(input.title, theme::header_style()),
                Line::from(""),
                Line::from(format!("{}: {}", input.prompt, input.value)),
                Line::from("Enter submit | Esc cancel"),
            ];

            let widget = Paragraph::new(lines)
                .block(
                    Block::bordered()
                        .title("Input")
                        .border_style(theme::focus_border_style()),
                )
                .wrap(Wrap { trim: false });
            frame.render_widget(widget, area);
        }
        Modal::AddProject(add) => {
            let area = centered_rect(70, 36, frame.area());
            frame.render_widget(Clear, area);

            let path_prefix = if add.active_field == AddProjectField::Path {
                ">"
            } else {
                " "
            };
            let name_prefix = if add.active_field == AddProjectField::Name {
                ">"
            } else {
                " "
            };

            let lines = vec![
                Line::styled("Add project", theme::header_style()),
                Line::from(""),
                Line::from(format!("{path_prefix} Path: {}", add.path)),
                Line::from(format!("{name_prefix} Name (optional): {}", add.name)),
                Line::from("Tab switch field | Enter submit | Esc cancel"),
            ];

            let widget = Paragraph::new(lines)
                .block(
                    Block::bordered()
                        .title("Project")
                        .border_style(theme::focus_border_style()),
                )
                .wrap(Wrap { trim: false });

            frame.render_widget(widget, area);
        }
        Modal::Confirm(confirm) => {
            let area = centered_rect(60, 26, frame.area());
            frame.render_widget(Clear, area);

            let lines = vec![
                Line::styled(confirm.title, theme::header_style()),
                Line::from(""),
                Line::from(confirm.message),
                Line::from(""),
                Line::from("Enter/Y confirm | N/Esc cancel"),
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
