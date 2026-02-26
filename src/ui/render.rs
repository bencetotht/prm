use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::state::{AddProjectField, AppState, FocusPane, Modal};
use crate::fs::agents::AgentsContent;
use crate::git::GitHistory;
use crate::ui::layout::{centered_rect, split_main, split_right_column};
use crate::ui::theme;
use crate::ui::widgets::pane_block;

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let (panes, footer) = split_main(frame.area());
    let (right_top, right_bottom) = split_right_column(panes[2]);

    render_projects(frame, app, panes[0]);
    render_todos(frame, app, panes[1]);
    render_agents(frame, app, right_top);
    render_git_history(frame, app, right_bottom);
    render_footer(frame, app, footer);

    if app.show_help {
        render_help_overlay(frame);
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
            ListItem::new(Line::from(vec![
                Span::raw(format!("{marker} ")),
                Span::styled(
                    format!("[{}]", git_status.short_label()),
                    theme::git_status_style(&git_status),
                ),
                Span::raw(format!(" {}", project.name)),
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

fn render_todos(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let block = pane_block("[2] Todos", app.focus == FocusPane::Todos);

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
            let style = if todo.done {
                theme::done_todo_style()
            } else {
                Style::default()
            };
            ListItem::new(Line::styled(format!("{check} {}", todo.title), style))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_item_style())
        .highlight_symbol("▌ ");

    let mut state = ListState::default();
    state.select(Some(app.selected_todo));
    frame.render_stateful_widget(list, area, &mut state);
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
    let body = match app.current_git_history() {
        GitHistory::NotGit => vec![Line::from("Not a git repository")],
        GitHistory::Empty => vec![Line::from("No commits yet")],
        GitHistory::Error(err) => vec![Line::from(format!("Read error: {err}"))],
        GitHistory::Lines(lines) => lines.into_iter().map(Line::from).collect(),
    };

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
            "{} | 1-4 panes | arrows/hjkl move | Tab panes | mouse click/scroll | a/r/x/d project | n/e/space/dd todo | ? help | Q quit",
            app.status
        )
    };

    let paragraph = Paragraph::new(content)
        .style(theme::status_style())
        .wrap(Wrap { trim: true })
        .block(Block::default());
    frame.render_widget(paragraph, area);
}

fn render_help_overlay(frame: &mut Frame<'_>) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);

    let lines = vec![
        Line::styled("PRM Keymap", theme::header_style()),
        Line::from(""),
        Line::styled("Global", theme::header_style()),
        Line::from(
            "1/2/3/4 focus pane, arrows or h/j/k/l move, Tab/Shift-Tab pane, mouse click/scroll, / filter, q close, Q quit, ? help",
        ),
        Line::from(""),
        Line::styled("Projects pane", theme::header_style()),
        Line::from("a add, r rename, x archive/unarchive, d delete(confirm), A toggle archived"),
        Line::from(
            "Git badge legend: CHG changed, PUSH waiting to push, COMMIT local-only, OK synced",
        ),
        Line::from(""),
        Line::styled("Todos pane", theme::header_style()),
        Line::from("n new, e/Enter edit, Space toggle done, dd delete, J/K reorder"),
        Line::from(""),
        Line::styled("AGENTS pane", theme::header_style()),
        Line::from("j/k scroll content"),
        Line::from(""),
        Line::styled("Git history pane", theme::header_style()),
        Line::from("Tab/4 to focus, arrows or j/k scroll commits"),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::bordered()
                .title("Help")
                .title_style(theme::header_style())
                .border_style(theme::focus_border_style()),
        )
        .style(theme::help_style())
        .wrap(Wrap { trim: false });

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
