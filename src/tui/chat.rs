//! 聊天消息区渲染（含流式消息、思考过程与 toast 浮层）。

use crate::app::{App, Focus, Generation};
use crate::model::{Message, Role};
use crate::tui::markdown;
use crate::tui::theme::Theme;
use crate::util::{truncate_by_width, wrap_text};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

/// 选中消息头部的高亮背景。
const SEL_BG: Color = Color::Rgb(46, 52, 64);

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme;
    let focused = app.focus == Focus::Chat;
    let title = app
        .current_session()
        .map(|s| truncate_by_width(&s.title, 30))
        .unwrap_or_default();
    let border = if focused {
        Style::new().fg(theme.accent)
    } else {
        Style::new().fg(theme.faint)
    };
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(theme.dim),
        ))
        .border_style(border);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // 正文渲染宽度（右侧留 1 列呼吸空间）
    let width = inner.width.saturating_sub(1) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut ranges: Vec<(u16, u16)> = Vec::with_capacity(app.view.messages.len());
    for (i, m) in app.view.messages.iter().enumerate() {
        let start = lines.len() as u16;
        let selected = focused && app.chat_sel == Some(i);
        append_message(&mut lines, m, width, &theme, selected);
        ranges.push((start, lines.len() as u16));
    }
    if let Some(gen) = &app.generating {
        append_streaming(&mut lines, gen, width, &theme);
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "（暂无消息）发送第一条消息开始对话 · ? 查看帮助",
            Style::new().fg(theme.faint),
        )));
    }

    let total = lines.len() as u16;
    app.chat_total = total;
    app.chat_height = inner.height;
    app.chat_msg_ranges = ranges;
    // None = 跟随底部；Some = 固定偏移
    let offset = match app.chat_scroll {
        None => total.saturating_sub(inner.height),
        Some(o) => o.min(total.saturating_sub(inner.height)),
    };
    f.render_widget(Paragraph::new(lines).scroll((offset, 0)), inner);

    // toast 浮层（聊天区顶部一行）
    if let Some(t) = &app.toast {
        let rect = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Clear, rect);
        let (sym, color) = if t.error {
            ("✖ ", theme.error)
        } else {
            ("✔ ", theme.accent)
        };
        let text = truncate_by_width(&t.text, inner.width.saturating_sub(4) as usize);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(sym, Style::new().fg(color)),
                Span::styled(text, Style::new().fg(color)),
            ])),
            rect,
        );
    }
}

/// 追加一条持久化消息。
fn append_message(
    lines: &mut Vec<Line<'static>>,
    m: &Message,
    width: usize,
    theme: &Theme,
    selected: bool,
) {
    let (label, color) = match m.role {
        Role::User => ("❯ 你", theme.user),
        Role::Assistant => ("❯ AI", theme.assistant),
        Role::System => ("❯ 系统", theme.dim),
    };
    let mut head = Style::new().fg(color).add_modifier(Modifier::BOLD);
    if selected {
        head = head.bg(SEL_BG);
    }
    lines.push(Line::from(Span::styled(label, head)));
    if let Some(r) = &m.reasoning {
        if !r.is_empty() {
            append_reasoning(lines, r, width, theme);
        }
    }
    match m.role {
        Role::User => {
            for l in wrap_text(&m.content, width.saturating_sub(2)) {
                lines.push(Line::from(format!("  {l}")));
            }
        }
        _ => {
            lines.extend(markdown::render(&m.content, width, theme));
        }
    }
    lines.push(Line::from(""));
}

/// 追加思考过程块（暗色）。
fn append_reasoning(lines: &mut Vec<Line<'static>>, r: &str, width: usize, theme: &Theme) {
    lines.push(Line::from(Span::styled(
        "┄ 思考过程",
        Style::new()
            .fg(theme.reasoning)
            .add_modifier(Modifier::ITALIC),
    )));
    for l in wrap_text(r, width.saturating_sub(2)) {
        lines.push(Line::from(Span::styled(
            format!("  {l}"),
            Style::new().fg(theme.reasoning),
        )));
    }
}

/// 追加生成中的流式消息。
fn append_streaming(lines: &mut Vec<Line<'static>>, gen: &Generation, width: usize, theme: &Theme) {
    lines.push(Line::from(Span::styled(
        "❯ AI",
        Style::new()
            .fg(theme.assistant)
            .add_modifier(Modifier::BOLD),
    )));
    if !gen.reasoning.is_empty() {
        append_reasoning(lines, &gen.reasoning, width, theme);
    }
    let text = if gen.content.is_empty() {
        "…"
    } else {
        gen.content.as_str()
    };
    lines.extend(markdown::render(text, width, theme));
    lines.push(Line::from(Span::styled(
        "▍ 生成中…",
        Style::new().fg(theme.faint),
    )));
    lines.push(Line::from(""));
}
