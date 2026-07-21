use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus, Overlay, PortSide, RunState};
use crate::config::ConfigField;

pub fn render(app: &App, frame: &mut Frame) {
    let outer = Block::bordered().title(" COMsniff ");
    let inner = outer.inner(frame.area());
    frame.render_widget(outer, frame.area());

    let [config_row, com_row, log_area, bottom_row] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .areas(inner);

    render_config_row(app, frame, config_row);
    render_com_row(app, frame, com_row);
    render_log_view(app, frame, log_area);
    render_bottom_row(app, frame, bottom_row);

    match app.overlay {
        Overlay::PortDropdown { side, highlighted } => render_dropdown(app, frame, com_row, side, highlighted),
        Overlay::Config { field } => render_config_modal(app, frame, field),
        Overlay::None => {}
    }
}

fn focus_style(is_focused: bool, enabled: bool) -> Style {
    if !enabled {
        Style::new().add_modifier(Modifier::DIM)
    } else if is_focused {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new()
    }
}

fn render_config_row(app: &App, frame: &mut Frame, area: Rect) {
    let [button_area, status_area] = Layout::horizontal([Constraint::Length(12), Constraint::Min(0)]).areas(area);

    let block = Block::bordered().border_style(focus_style(app.focus == Focus::Config, true));
    frame.render_widget(Paragraph::new("Config").block(block), button_area);

    if let Some(msg) = &app.status_message {
        let line = Paragraph::new(msg.as_str()).style(Style::new().fg(Color::Yellow));
        frame.render_widget(line, status_area);
    } else if let Some(err) = &app.port_enum_error {
        let warn = Paragraph::new(format!("could not list ports: {err}")).style(Style::new().fg(Color::Red));
        frame.render_widget(warn, status_area);
    }
}

fn render_com_row(app: &App, frame: &mut Frame, area: Rect) {
    let [port_left, start_stop, port_right] = split_com_row(area);

    let port_left_label = port_label(app, app.port_left_selected, "Select port");
    let port_left_block =
        Block::bordered().border_style(focus_style(app.focus == Focus::PortLeftSelector, true));
    frame.render_widget(Paragraph::new(port_left_label).block(port_left_block), port_left);

    let start_stop_label = match app.run_state {
        RunState::Stopped => "Start",
        RunState::Running => "Stop",
    };
    let start_stop_block = Block::bordered().border_style(focus_style(app.focus == Focus::StartStop, true));
    frame.render_widget(Paragraph::new(start_stop_label).block(start_stop_block), start_stop);

    let port_right_label = port_label(app, app.port_right_selected, "Select port");
    let port_right_block =
        Block::bordered().border_style(focus_style(app.focus == Focus::PortRightSelector, true));
    frame.render_widget(Paragraph::new(port_right_label).block(port_right_block), port_right);
}

fn port_label(app: &App, selected: Option<usize>, placeholder: &str) -> String {
    selected
        .and_then(|i| app.available_ports.get(i))
        .cloned()
        .unwrap_or_else(|| placeholder.to_string())
}

fn split_com_row(area: Rect) -> [Rect; 3] {
    Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)]).areas(area)
}

fn render_log_view(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::bordered();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let visible = (inner.height as usize).max(1);
    let total = app.log_lines.len();
    let end = total.saturating_sub(app.log_scroll);
    let start = end.saturating_sub(visible);

    let items: Vec<ListItem> = app.log_lines[start..end]
        .iter()
        .map(|line| {
            let color = match line.direction {
                crate::app::Direction::Outgoing => Color::Green,
                crate::app::Direction::Incoming => Color::Cyan,
            };
            ListItem::new(format!("{} {}", line.direction.arrow(), line.text)).style(Style::new().fg(color))
        })
        .collect();

    frame.render_widget(List::new(items), inner);
}

fn render_bottom_row(app: &App, frame: &mut Frame, area: Rect) {
    let [checkbox_area, path_area] = Layout::horizontal([Constraint::Length(12), Constraint::Min(0)]).areas(area);

    let checkbox_enabled = app.is_enabled(Focus::LogCheckbox);
    let mark = if app.log_enabled { "[x]" } else { "[ ]" };
    let checkbox_style = focus_style(app.focus == Focus::LogCheckbox, checkbox_enabled);
    frame.render_widget(Paragraph::new(format!("{mark} Log")).style(checkbox_style), checkbox_area);

    let path_enabled = app.is_enabled(Focus::LogPath);
    let path_block = Block::bordered().border_style(focus_style(app.focus == Focus::LogPath, path_enabled));
    let path_inner = path_block.inner(path_area);
    let path_style = if path_enabled { Style::new() } else { Style::new().add_modifier(Modifier::DIM) };
    frame.render_widget(Paragraph::new(app.log_path.as_str()).style(path_style).block(path_block), path_area);

    let path_focused = app.focus == Focus::LogPath && matches!(app.overlay, Overlay::None);
    if path_focused && path_inner.width > 0 {
        let cursor_x = path_inner.x + (app.log_path_cursor as u16).min(path_inner.width - 1);
        frame.set_cursor_position((cursor_x, path_inner.y));
    }
}

fn render_dropdown(app: &App, frame: &mut Frame, com_row: Rect, side: PortSide, highlighted: usize) {
    let [port_left, _start_stop, port_right] = split_com_row(com_row);
    let anchor = match side {
        PortSide::Left => port_left,
        PortSide::Right => port_right,
    };

    let width = anchor.width.max(16);
    let height = ((app.available_ports.len() as u16) + 2).clamp(3, 10);
    let area = Rect { x: anchor.x, y: anchor.y + anchor.height, width, height };

    frame.render_widget(Clear, area);
    let block = Block::bordered().title("Select port");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.available_ports.is_empty() {
        frame.render_widget(Paragraph::new("(no ports found)"), inner);
        return;
    }

    let excluded = match side {
        PortSide::Left => app.port_right_selected,
        PortSide::Right => app.port_left_selected,
    };

    let items: Vec<ListItem> = app
        .available_ports
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == highlighted {
                Style::new().bg(Color::Yellow).fg(Color::Black)
            } else if Some(i) == excluded {
                Style::new().add_modifier(Modifier::DIM)
            } else {
                Style::new()
            };
            ListItem::new(name.as_str()).style(style)
        })
        .collect();

    frame.render_widget(List::new(items), inner);
}

fn render_config_modal(app: &App, frame: &mut Frame, field: ConfigField) {
    let area = popup_area(frame.area(), 44, 11);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title("Config");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut constraints = vec![Constraint::Length(1); ConfigField::ALL.len()];
    constraints.push(Constraint::Min(1));
    constraints.push(Constraint::Length(1));
    let rows = Layout::vertical(constraints).split(inner);

    for (row_area, f) in rows.iter().zip(ConfigField::ALL) {
        let style = if f == field { Style::new().fg(Color::Yellow) } else { Style::new() };
        let text = format!("{:<10} {}", f.label(), app.config.field_value_label(f));
        frame.render_widget(Paragraph::new(text).style(style), *row_area);
    }

    let flow_control_area = rows[ConfigField::ALL.len()];
    let flow_control = Paragraph::new("Flow control: not yet implemented").style(Style::new().add_modifier(Modifier::DIM));
    frame.render_widget(flow_control, flow_control_area);

    let controls_area = rows[ConfigField::ALL.len() + 1];
    let controls = Paragraph::new("\u{2191}/\u{2193} or Tab: field   \u{2190}/\u{2192}: change value   Enter/Esc: close")
        .style(Style::new().add_modifier(Modifier::DIM));
    frame.render_widget(controls, controls_area);
}

fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [area] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(area);
    area
}
