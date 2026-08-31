//! 底部状态栏：提供商/模型、生成状态、键位提示。

use crate::app::{App, Focus};
use crate::util::display_width;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

const BAR_BG: Color = Color::Rgb(46, 52, 64);

/// 按当前焦点/状态给出紧凑的键位提示（一行内）。
fn context_hint(app: &App) -> &'static str {
    if app.generating.is_some() {
        return " Esc 停止生成 ";
    }
    match app.focus {
        Focus::Input => " Enter 发送 · / 命令 · ? 帮助 ",
        Focus::Chat => " j/k 选消息 · e 编辑 · dd 删除 · c 复制代码 ",
        Focus::Sidebar => " Enter 切换 · n 新建 · r 重命名 · d 删除 ",
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    f.render_widget(Block::new().style(Style::new().bg(BAR_BG)), area);

    let hint = context_hint(app);
    let hint_w = display_width(hint) as u16;
    let cols = Layout::horizontal([Constraint::Min(10), Constraint::Length(hint_w)]).split(area);

    let mut spans = Vec::new();
    if let Some(s) = app.current_session() {
        let star = if app.config.is_favorite(&s.provider, &s.model) {
            "★ "
        } else {
            ""
        };
        spans.push(Span::styled(
            format!(" {} · {}{}", s.provider, star, s.model),
            Style::new().fg(theme.accent),
        ));
    }
    let state = if app.generating.is_some() {
        Some("⟳ 生成中".to_string())
    } else if app.models_loading {
        Some("⟳ 拉取模型".to_string())
    } else if !app.queue.is_empty() {
        Some(format!("⏳ 队列 {}", app.queue.len()))
    } else {
        None
    };
    if let Some(s) = state {
        spans.push(Span::styled(format!("  {s}"), Style::new().fg(theme.dim)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), cols[0]);
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::new().fg(theme.faint))),
        cols[1],
    );
}
