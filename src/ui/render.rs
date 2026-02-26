use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::state::{AddProjectField, AppState, FocusPane, Modal};
use crate::fs::agents::AgentsContent;
use crate::ui::layout::{centered_rect, split_main};
use crate::ui::theme;
use crate::ui::widgets::pane_block;

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let (panes, footer) = split_main(frame.area());

    render_projects(frame, app, panes[0]);
    render_todos(frame, app, panes[1]);
    render_agents(frame, app, panes[2]);
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
        "Projects"
    } else {
        "Projects (filtered)"
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
            ListItem::new(Line::from(format!("{marker} {}", project.name)))
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
    let block = pane_block("Todos", app.focus == FocusPane::Todos);

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
    let block = pane_block("AGENTS.md", app.focus == FocusPane::Agents);

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

fn render_footer(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let content = if app.filter_mode {
        format!("Filter: {}", app.filter_input)
    } else {
        format!(
            "{} | h/j/k/l move | Tab panes | a/r/x/d project | n/e/space/dd todo | ? help | Q quit",
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
        Line::from("h/j/k/l move, Tab/Shift-Tab pane, / filter, q close, Q quit, ? help"),
        Line::from(""),
        Line::styled("Projects pane", theme::header_style()),
        Line::from("a add, r rename, x archive/unarchive, d delete(confirm), A toggle archived"),
        Line::from(""),
        Line::styled("Todos pane", theme::header_style()),
        Line::from("n new, e/Enter edit, Space toggle done, dd delete, J/K reorder"),
        Line::from(""),
        Line::styled("AGENTS pane", theme::header_style()),
        Line::from("j/k scroll content"),
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
