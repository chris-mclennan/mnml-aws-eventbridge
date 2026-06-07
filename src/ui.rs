//! ratatui rendering + the main event loop.

use crate::app::{App, TabState};
use crate::eventbridge::Item;
use crate::keys;
use anyhow::Result;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};
use std::io::Stdout;
use std::time::Duration;

pub async fn run(app: &mut App) -> Result<()> {
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = event_loop(&mut terminal, app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;
        app.tick();
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == event::KeyEventKind::Press
            && let Some(action) = keys::handle(key, app)
        {
            let quit = keys::apply(action, app).await;
            if quit {
                break;
            }
        }
    }
    Ok(())
}

pub fn draw(f: &mut Frame, app: &App) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(size);
    draw_tabs(f, chunks[0], app);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);
    draw_list(f, body[0], app.active());
    draw_detail(
        f,
        body[1],
        app.focused_item(),
        app.focused_targets.as_ref().map(|(_, t)| t.as_slice()),
    );
    draw_status(f, chunks[2], app);
}

fn draw_tabs(f: &mut Frame, area: Rect, app: &App) {
    let labels: Vec<Line> = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let badge = if t.data.loading {
                " (…)".to_string()
            } else if t.data.last_error.is_some() {
                " (err)".to_string()
            } else {
                format!(" ({})", t.data.items.len())
            };
            Line::from(format!("{}.{}{}", i + 1, t.name, badge))
        })
        .collect();
    let tabs = Tabs::new(labels)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" eventbridge "),
        )
        .select(app.active_tab)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}

fn draw_list(f: &mut Frame, area: Rect, tab: &TabState) {
    if let Some(err) = &tab.data.last_error {
        let p = Paragraph::new(format!("error: {err}"))
            .style(Style::default().fg(Color::Red))
            .block(Block::default().borders(Borders::ALL).title(" items "));
        f.render_widget(p, area);
        return;
    }
    if tab.data.items.is_empty() {
        let msg = if tab.data.loading {
            "(loading…)"
        } else {
            "(none)"
        };
        let p = Paragraph::new(msg)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" items "));
        f.render_widget(p, area);
        return;
    }
    let body_rows = area.height.saturating_sub(2) as usize;
    let total = tab.data.items.len();
    let selected = tab.data.selected;
    let start = if total <= body_rows {
        0
    } else {
        let lo = selected.saturating_sub(body_rows / 2);
        lo.min(total - body_rows)
    };

    let lines: Vec<Line> = tab.data.items[start..]
        .iter()
        .take(body_rows)
        .enumerate()
        .map(|(i, item)| {
            let abs = start + i;
            let cursor = if abs == selected { "▸ " } else { "  " };
            let primary = truncate(item.primary_label(), 28);
            let secondary = item.secondary_label();
            let line = format!("{cursor}{:<28}  {secondary}", primary);
            let style = if abs == selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                state_color_for(item)
            };
            Line::from(Span::styled(line, style))
        })
        .collect();

    let title = match tab.spec.kind.as_str() {
        "buses" => format!(" buses ({total}) "),
        "rules" => format!(
            " rules · {} ({total}) ",
            tab.spec.event_bus_name.as_deref().unwrap_or("?")
        ),
        _ => format!(" items ({total}) "),
    };
    let p = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(p, area);
}

fn state_color_for(item: &Item) -> Style {
    match item {
        Item::Bus(_) => Style::default().fg(Color::Gray),
        Item::Rule(r) => match r.state.as_deref() {
            Some("DISABLED") => Style::default().fg(Color::DarkGray),
            Some("ENABLED") => Style::default().fg(Color::Gray),
            _ => Style::default().fg(Color::Gray),
        },
    }
}

fn draw_detail(
    f: &mut Frame,
    area: Rect,
    item: Option<&Item>,
    targets: Option<&[crate::eventbridge::Target]>,
) {
    let title = " detail ";
    let Some(item) = item else {
        let p = Paragraph::new("(no item selected)")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(title));
        f.render_widget(p, area);
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    let kv = |k: &str, v: String| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!(" {k:<15}"), Style::default().fg(Color::DarkGray)),
            Span::styled(v, Style::default().fg(Color::White)),
        ])
    };

    match item {
        Item::Bus(b) => {
            lines.push(kv("Name", b.name.clone()));
            if let Some(c) = b.creation_time {
                lines.push(kv("Created", fmt_epoch(c)));
            }
            if let Some(m) = b.last_modified_time {
                lines.push(kv("Last modified", fmt_epoch(m)));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                " ARN ",
                Style::default().fg(Color::DarkGray),
            )]));
            lines.push(Line::from(Span::styled(
                format!(" {}", b.arn),
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            )));
            if let Some(policy) = &b.policy
                && !policy.is_empty()
            {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    " Policy (JSON) ",
                    Style::default().fg(Color::DarkGray),
                )]));
                for ln in policy.lines().take(15) {
                    lines.push(Line::from(Span::styled(
                        format!(" {ln}"),
                        Style::default().fg(Color::Gray),
                    )));
                }
            }
        }
        Item::Rule(r) => {
            lines.push(kv("Name", r.name.clone()));
            lines.push(kv("State", r.state.clone().unwrap_or_else(|| "—".into())));
            if let Some(bus) = &r.event_bus_name {
                lines.push(kv("Bus", bus.clone()));
            }
            if let Some(sched) = &r.schedule_expression {
                lines.push(kv("Schedule", sched.clone()));
            }
            if let Some(role) = &r.role_arn {
                lines.push(kv("Role", short_arn(role)));
            }
            if let Some(mgr) = &r.managed_by {
                lines.push(kv("Managed by", mgr.clone()));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                " ARN ",
                Style::default().fg(Color::DarkGray),
            )]));
            lines.push(Line::from(Span::styled(
                format!(" {}", r.arn),
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            )));
            if let Some(desc) = &r.description
                && !desc.is_empty()
            {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    " Description ",
                    Style::default().fg(Color::DarkGray),
                )]));
                lines.push(Line::from(Span::styled(
                    format!(" {desc}"),
                    Style::default().fg(Color::Gray),
                )));
            }
            if let Some(pat) = &r.event_pattern
                && !pat.is_empty()
            {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    " Event pattern ",
                    Style::default().fg(Color::DarkGray),
                )]));
                for ln in pat.lines().take(15) {
                    lines.push(Line::from(Span::styled(
                        format!(" {ln}"),
                        Style::default().fg(Color::Gray),
                    )));
                }
            }
            // Targets: every entry `list-targets-by-rule` returned for
            // this rule. One row per target showing service + truncated
            // ARN tail; a Input snippet line below when present.
            if let Some(targets) = targets {
                lines.push(Line::from(""));
                let header = if targets.is_empty() {
                    " Targets (0) ".to_string()
                } else {
                    format!(" Targets ({}) ", targets.len())
                };
                lines.push(Line::from(vec![Span::styled(
                    header,
                    Style::default().fg(Color::DarkGray),
                )]));
                if targets.is_empty() {
                    lines.push(Line::from(Span::styled(
                        " (none — rule has no targets configured)",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::DIM),
                    )));
                } else {
                    for t in targets.iter().take(8) {
                        let service = t.service();
                        let arn_tail = t
                            .arn
                            .rsplit(':')
                            .next()
                            .filter(|s| !s.is_empty())
                            .unwrap_or(&t.arn);
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!(" {service:<10} "),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::styled(arn_tail.to_string(), Style::default().fg(Color::Gray)),
                        ]));
                        if let Some(input) = &t.input {
                            let snippet: String = input.chars().take(60).collect();
                            let suffix = if input.chars().count() > 60 {
                                "…"
                            } else {
                                ""
                            };
                            lines.push(Line::from(Span::styled(
                                format!("     input: {snippet}{suffix}"),
                                Style::default()
                                    .fg(Color::DarkGray)
                                    .add_modifier(Modifier::DIM),
                            )));
                        }
                    }
                    if targets.len() > 8 {
                        lines.push(Line::from(Span::styled(
                            format!("     … {} more", targets.len() - 8),
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::DIM),
                        )));
                    }
                }
            }
        }
    }

    let p = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(p, area);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let hint =
        " 1-9 tab · ↑↓/jk move · o console · y ARN · L jump (lambda/sqs/sns) · r refresh · q quit ";
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", app.status),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            hint,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn short_arn(arn: &str) -> String {
    arn.rsplit('/').next().unwrap_or(arn).to_string()
}

/// Format an EventBridge unix-epoch (float) as a short date.
fn fmt_epoch(f: f64) -> String {
    use chrono::DateTime;
    DateTime::from_timestamp(f as i64, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| format!("{f}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_strings_unchanged() {
        assert_eq!(truncate("short", 10), "short");
    }

    #[test]
    fn truncate_long_strings_get_ellipsis() {
        let out = truncate("0123456789abcdef", 8);
        assert_eq!(out.chars().count(), 8);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn short_arn_extracts_last_segment() {
        let arn = "arn:aws:iam::123456789012:role/my-role";
        assert_eq!(short_arn(arn), "my-role");
    }

    #[test]
    fn fmt_epoch_renders_known_date() {
        // 2024-01-01T00:00:00 UTC
        let out = fmt_epoch(1_704_067_200.0);
        assert!(out.starts_with("2024-01-01"));
    }
}
