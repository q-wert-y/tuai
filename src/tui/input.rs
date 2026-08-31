//! 多行输入框渲染与高度计算。

use crate::app::App;
use crate::util::{display_width, wrap_text};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// 输入区高度：边框 3 行 + 文本行（上限 5），即 3..=8。
pub fn height(app: &App, width: u16) -> u16 {
    let inner_w = (width as usize).saturating_sub(2).max(1);
    let text = app.input.lines.join("\n");
    if text.is_empty() {
        return 3;
    }
    let rows = wrap_text(&text, inner_w).len() as u16;
    3 + rows.min(5)
}

pub fn render(f: &mut Frame, area: Rect, app: &App) -> Option<(u16, u16)> {
    let theme = &app.theme;
    let focused = app.focus == crate::app::Focus::Input
        && app.palette.is_none()
        && app.form.is_none()
        && !app.show_help;
    let border = if focused {
        Style::new().fg(theme.accent)
    } else {
        Style::new().fg(theme.faint)
    };
    let title = if app.generating.is_some() {
        " 输入（生成中 · Esc 停止）"
    } else {
        " 输入 "
    };
    let block = Block::bordered()
        .title(Span::styled(title, Style::new().fg(theme.dim)))
        .border_style(border);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner_w = inner.width as usize;

    if app.input.is_empty() {
        let hint = if app.generating.is_some() {
            "生成中… 可继续输入，完成后自动发送"
        } else {
            "输入消息 · / 命令面板 · ? 帮助 · Shift+Enter 换行"
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(hint, Style::new().fg(theme.faint)))),
            inner,
        );
        if focused {
            return Some((inner.x, inner.y));
        }
        return None;
    }

    let mut lines: Vec<Line> = Vec::new();
    for l in &app.input.lines {
        if l.is_empty() {
            lines.push(Line::from(""));
            continue;
        }
        for seg in wrap_text(l, inner_w) {
            lines.push(Line::from(Span::raw(seg)));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);

    if !focused {
        return None;
    }
    // 光标定位：当前行内按显示宽度折算
    let mut row: u16 = 0;
    for (i, l) in app.input.lines.iter().enumerate() {
        let wrapped = wrap_text(l, inner_w).len().max(1) as u16;
        if i == app.input.cy {
            let byte = char_to_byte(l, app.input.cx);
            let before = display_width(&l[..byte]);
            let r = (before / inner_w.max(1)) as u16;
            let c = (before % inner_w.max(1)) as u16;
            let x = inner.x + c;
            let y = (inner.y + row + r).min(inner.y + inner.height.saturating_sub(1));
            return Some((x, y));
        }
        row += wrapped;
    }
    Some((inner.x, inner.y))
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}
